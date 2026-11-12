// Which file a path in a reply names (the Swift app's FilePathRefs): agents
// write paths in inline code (`app/Sources/Foo.swift`, `TranscriptLinkTests.swift`,
// `/tmp/a.txt:12`) far more than as links, and name a file by itself once
// they've been in its repo. Which file one means is only worked out when it's
// clicked, pointed at or right-clicked, from the thread's folder and the
// files its tools read and changed, because files come and go and a reply's
// rendering is kept by its text.
use crate::reducer::{Row, RowContent};
use std::path::Path;

/// `Foo.swift:12` or `Foo.swift:12:4` names the file, and a line in it.
pub fn strip_line(text: &str) -> &str {
    let mut path = text;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, tail)) if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => path = head,
            _ => break,
        }
    }
    path
}

/// `grow.html?clean&s=45` or `guide.md#links`: a file, with a query or
/// fragment that's for the page (its own script reads `?clean`), not part of
/// the file's name. Split at the first `?` or `#`.
pub fn split_query(text: &str) -> (&str, Option<&str>, Option<&str>) {
    let Some(mark) = text.find(['?', '#']) else { return (text, None, None) };
    let (path, rest) = (&text[..mark], &text[mark + 1..]);
    if text.as_bytes()[mark] == b'#' {
        return (path, None, Some(rest));
    }
    match rest.split_once('#') {
        Some((query, fragment)) => (path, Some(query), Some(fragment)),
        None => (path, Some(rest), None),
    }
}

/// The file a path in a reply names, if one's there: as written (with `~`),
/// then from the thread's folder, then from what the thread's tools touched,
/// most recent first (a path ending with it, or it beside a file they
/// touched or inside a folder they `cd`-ed into), then a look through those
/// folders for it (`FilePathRefs.resolve`). `search` looks for a path under
/// a folder (`search` below, on this computer).
pub fn resolve(text: &str, root: Option<&str>, known: &[String], home: &str, exists: &dyn Fn(&str) -> bool, search: &dyn Fn(&str, &str) -> Option<String>) -> Option<String> {
    let mut path = strip_line(text).to_owned();
    if path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        path = format!("{}/{rest}", home.trim_end_matches('/'));
    }
    if path.starts_with('/') || crate::reducer::text::is_windows_path(&path) {
        return exists(&path).then_some(path);
    }
    let join = |base: &str| standardize(&format!("{}/{}", base.trim_end_matches('/'), path));
    if let Some(root) = root.filter(|r| !r.is_empty()) {
        let joined = join(root);
        if exists(&joined) {
            return Some(joined);
        }
    }
    let trimmed = path.strip_prefix("./").unwrap_or(&path);
    for candidate in known.iter().rev() {
        if (candidate == trimmed || candidate.replace('\\', "/").ends_with(&format!("/{}", trimmed.replace('\\', "/")))) && exists(candidate) {
            return Some(candidate.clone());
        }
        for base in [candidate.as_str(), parent(candidate)] {
            let joined = join(base);
            if exists(&joined) {
                return Some(joined);
            }
        }
    }
    // Last, a look through those folders for it: four at most, as it runs
    // when Mike points at it.
    let mut searched: Vec<String> = Vec::new();
    let folders = root.filter(|r| !r.is_empty()).map(str::to_owned).into_iter().chain(known.iter().rev().map(|k| if exists(&format!("{k}/.")) { k.clone() } else { parent(k).to_owned() }));
    for folder in folders {
        if searched.len() >= 4 {
            break;
        }
        if searched.contains(&folder) {
            continue;
        }
        searched.push(folder.clone());
        if let Some(found) = search(&folder, trimmed) {
            return Some(found);
        }
    }
    None
}

/// The file `text` names, with any `?query` and `#fragment` the agent put
/// after it for the page (`FilePathRefs.fileLink`). A file whose own name
/// has a `?` or `#` in it wins over reading that as a query.
pub fn file_link(text: &str, root: Option<&str>, known: &[String], home: &str, exists: &dyn Fn(&str) -> bool, search: &dyn Fn(&str, &str) -> Option<String>) -> Option<FileLink> {
    let (path, query, fragment) = split_query(text);
    if (query.is_none() && fragment.is_none()) || path.is_empty() {
        return resolve(text, root, known, home, exists, search).map(|path| FileLink { path, query: None, fragment: None });
    }
    // The whole text as a name: no folder search for it, as the split one
    // gets that.
    if let Some(path) = resolve(text, root, known, home, exists, &|_, _| None) {
        return Some(FileLink { path, query: None, fragment: None });
    }
    resolve(path, root, known, home, exists, search).map(|path| FileLink { path, query: query.map(str::to_owned), fragment: fragment.map(str::to_owned) })
}

/// A file a reply names, and what its page is opened with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

/// Folders a search never goes into: built and fetched files, and big.
const SKIPPED: &[&str] = &["node_modules", ".build", ".git", "dist", "build", "DerivedData", "Pods", ".next", "target"];

/// Bundles are files to the Finder, and nothing in them is a project's file.
const PACKAGES: &[&str] = &["app", "framework", "bundle", "xcodeproj", "xcworkspace", "xcassets", "photoslibrary", "rtfd", "pkg", "plugin", "kext", "appex"];

/// The first file under `folder` whose path ends with `path`, looking at no
/// more than a few thousand entries so a click stays quick (`FilePathRefs.search`).
/// Hidden files and bundles' insides are passed over.
pub fn search(folder: &str, path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.is_empty() {
        return None;
    }
    let suffix = format!("/{path}");
    let mut seen = 0usize;
    let mut stack = vec![Path::new(folder).to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = crate::open_policy::local_read_dir(&dir) else { continue };
        let mut children: Vec<_> = entries.into_iter().flatten().collect();
        // A steady order, so the same click finds the same file.
        children.sort_by_key(|e| e.file_name());
        let mut folders = Vec::new();
        for entry in children {
            seen += 1;
            if seen > 20_000 {
                return None;
            }
            let file_name = entry.file_name();
            let entry_name = file_name.to_string_lossy();
            if entry_name.starts_with('.') || SKIPPED.contains(&entry_name.as_ref()) {
                continue;
            }
            // Keep the spelling the caller browses (a /var or 8.3 alias),
            // even when Windows opened the directory by its resolved path.
            let full = dir.join(&file_name);
            if entry_name == name {
                let found = full.to_string_lossy().into_owned();
                // Windows' paths come back with backslashes, while what a
                // reply names has forward ones, so nothing matched there.
                if found.replace(std::path::MAIN_SEPARATOR, "/").ends_with(&suffix) && crate::open_policy::local_metadata(&full).is_ok() {
                    return Some(found);
                }
            }
            let is_package = entry_name.rsplit_once('.').is_some_and(|(_, ext)| PACKAGES.contains(&ext.to_lowercase().as_str()));
            if !is_package && entry.is_folder {
                folders.push(full);
            }
        }
        // Depth first, nearest folders looked at first.
        stack.extend(folders.into_iter().rev());
    }
    None
}

/// Absolute paths in a thread's tool calls, oldest first: the files they
/// read and changed, and the folders commands `cd` into (`FilePathRefs.toolPaths`).
pub fn tool_paths(items: &[Row], home: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for item in items {
        let RowContent::Tool(tool) = &item.content else { continue };
        let Some(call) = &tool.call else { continue };
        for key in ["file_path", "notebook_path", "path"] {
            if let Some(path) = call.input.get(key).and_then(|v| v.as_str()) {
                paths.push(path.to_owned());
            }
        }
        paths.extend(tool.edited_paths());
        if let Some(command) = call.input.get("command").and_then(|v| v.as_str()) {
            paths.extend(cd_targets(command, home));
        }
    }
    paths.retain(|p| p.starts_with('/') || crate::reducer::text::is_windows_path(p));
    paths
}

/// The folders `cd` moves to in a shell command, made absolute: a `cd` at
/// the start, or after `&&`, `;`, `||` or `(` (`FilePathRefs.cdTargets`).
pub fn cd_targets(command: &str, home: &str) -> Vec<String> {
    let bytes = command.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = command[at..].find("cd") {
        let start = at + offset;
        at = start + 2;
        // What's before it, spaces aside, has to start a command.
        let before = command[..start].trim_end_matches([' ', '\t']);
        let starts = before.is_empty() || before.ends_with("&&") || before.ends_with(';') || before.ends_with("||") || before.ends_with('(');
        if !starts || !bytes.get(start + 2).is_some_and(|b| *b == b' ' || *b == b'\t') {
            continue;
        }
        let rest = command[start + 2..].trim_start_matches([' ', '\t']);
        let dir = match rest.chars().next() {
            Some(quote @ ('"' | '\'')) => match rest[1..].find(quote) {
                Some(end) if end > 0 => rest[1..1 + end].to_owned(),
                _ => continue,
            },
            Some(_) => rest.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | ')')).next().unwrap_or("").to_owned(),
            None => continue,
        };
        if dir.is_empty() {
            continue;
        }
        let dir = if dir == "~" {
            home.to_owned()
        } else if let Some(rest) = dir.strip_prefix("~/") {
            format!("{}/{rest}", home.trim_end_matches('/'))
        } else {
            dir
        };
        if dir.starts_with('/') {
            found.push(dir);
        }
    }
    found
}

/// Another Mac's home folder, from its thread's folder (`remoteHome`): a
/// `/Users/<name>` there, else this computer's.
pub fn remote_home(cwd: &str, local_home: &str) -> String {
    let parts: Vec<&str> = cwd.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 && parts[0] == "Users" {
        return format!("/Users/{}", parts[1]);
    }
    local_home.to_owned()
}

/// `a/./b/../c` as `a/c` (`standardizingPath`).
fn standardize(path: &str) -> String {
    let normalized;
    let path = if crate::reducer::text::is_windows_path(path) {
        normalized = path.replace('\\', "/");
        normalized.as_str()
    } else { path };
    let rooted = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if rooted {
        format!("/{joined}")
    } else {
        joined
    }
}

fn parent(path: &str) -> &str {
    let sep = |c| c == '/' || (crate::reducer::text::is_windows_path(path) && c == '\\');
    match path.trim_end_matches(sep).rsplit_once(sep) {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => "",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_absolute_tool_paths_are_found_without_a_thread_folder() {
        let file = r"C:\work\notes.md";
        let exists = |p: &str| p == file;
        assert_eq!(super::resolve(file, Some(r"D:\other"), &[], "", &exists, &|_, _| None).as_deref(), Some(file));
        assert_eq!(super::resolve("notes.md", None, &[file.into()], "", &exists, &|_, _| None).as_deref(), Some(file));
    }
    use super::*;
    use crate::reducer::{ToolCall, ToolItem, ToolKind};
    use serde_json::json;

    fn no_search(_: &str, _: &str) -> Option<String> {
        None
    }

    #[test]
    fn lines_and_queries_come_off_the_name() {
        assert_eq!(strip_line("Foo.swift:12"), "Foo.swift");
        assert_eq!(strip_line("/tmp/a.txt:12:4"), "/tmp/a.txt");
        assert_eq!(strip_line("a:b"), "a:b");
        assert_eq!(split_query("grow.html?clean&s=45#top"), ("grow.html", Some("clean&s=45"), Some("top")));
        assert_eq!(split_query("guide.md#links"), ("guide.md", None, Some("links")));
        assert_eq!(split_query("plain.md"), ("plain.md", None, None));
    }

    #[test]
    fn a_path_is_found_as_written_from_the_folder_then_from_what_the_tools_touched() {
        let files = ["/w/repo/docs/plan.md", "/h/notes.md", "/w/repo/app/Sources/Foo.swift", "/w/other/README.md"];
        let exists = |p: &str| files.contains(&p) || p == "/w/repo/app/Sources/." || p == "/w/repo/.";
        let known: Vec<String> = vec!["/w/other/README.md".into(), "/w/repo/app/Sources/Foo.swift".into()];
        assert_eq!(resolve("/h/notes.md", None, &[], "/h", &exists, &no_search).as_deref(), Some("/h/notes.md"));
        assert_eq!(resolve("~/notes.md", None, &[], "/h", &exists, &no_search).as_deref(), Some("/h/notes.md"));
        assert_eq!(resolve("docs/plan.md:3", Some("/w/repo"), &[], "/h", &exists, &no_search).as_deref(), Some("/w/repo/docs/plan.md"));
        assert_eq!(resolve("./docs/../docs/plan.md", Some("/w/repo"), &[], "/h", &exists, &no_search).as_deref(), Some("/w/repo/docs/plan.md"));
        // A file the tools touched, by the end of its path.
        assert_eq!(resolve("Sources/Foo.swift", Some("/nowhere"), &known, "/h", &exists, &no_search).as_deref(), Some("/w/repo/app/Sources/Foo.swift"));
        // Beside a file they touched.
        assert_eq!(resolve("README.md", None, &known, "/h", &exists, &no_search).as_deref(), Some("/w/other/README.md"));
        assert_eq!(resolve("gone.md", Some("/w/repo"), &known, "/h", &exists, &no_search), None);
        assert_eq!(resolve("/nope.md", None, &[], "/h", &exists, &no_search), None);
    }

    #[test]
    fn a_name_by_itself_is_looked_for_in_the_folders_it_could_be_in() {
        let asked = std::cell::RefCell::new(Vec::new());
        let search = |folder: &str, path: &str| {
            asked.borrow_mut().push(format!("{folder}:{path}"));
            (folder == "/w/b" && path == "Deep.swift").then(|| "/w/b/x/y/Deep.swift".to_owned())
        };
        let known: Vec<String> = vec!["/w/a/one.rs".into(), "/w/b/two.rs".into()];
        let exists = |_: &str| false;
        assert_eq!(resolve("Deep.swift", Some("/w/root"), &known, "/h", &exists, &search).as_deref(), Some("/w/b/x/y/Deep.swift"));
        // Its own folder first, then the touched files' folders, newest first.
        assert_eq!(*asked.borrow(), ["/w/root:Deep.swift", "/w/b:Deep.swift"]);
    }

    #[test]
    fn a_query_or_fragment_stays_with_the_page() {
        let exists = |p: &str| p == "/w/grow.html" || p == "/w/odd#name.md";
        let link = file_link("grow.html?clean#top", Some("/w"), &[], "/h", &exists, &no_search).unwrap();
        assert_eq!(link, FileLink { path: "/w/grow.html".into(), query: Some("clean".into()), fragment: Some("top".into()) });
        // A name with a # in it is that file.
        assert_eq!(file_link("odd#name.md", Some("/w"), &[], "/h", &exists, &no_search).unwrap().path, "/w/odd#name.md");
        assert!(file_link("missing.html?x", Some("/w"), &[], "/h", &exists, &no_search).is_none());
    }

    #[test]
    fn a_search_finds_a_file_by_the_end_of_its_path_and_passes_builds_over() {
        let root = std::env::temp_dir().join(format!("tb-file-refs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["src/deep/down", "node_modules/x", ".hidden", "Tool.app/Contents"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["src/deep/down/Target.swift", "node_modules/x/Only.swift", ".hidden/Secret.swift", "Tool.app/Contents/Inside.swift"] {
            std::fs::write(root.join(file), "x").unwrap();
        }
        let folder = root.to_string_lossy().into_owned();
        // Joined a part at a time, so Windows' is the backslashed path it finds.
        let target = root.join("src").join("deep").join("down").join("Target.swift");
        assert_eq!(search(&folder, "Target.swift").as_deref(), Some(target.to_string_lossy().as_ref()));
        assert_eq!(search(&folder, "down/Target.swift").as_deref(), Some(target.to_string_lossy().as_ref()));
        assert_eq!(search(&folder, "up/Target.swift"), None);
        for passed_over in ["Only.swift", "Secret.swift", "Inside.swift"] {
            assert_eq!(search(&folder, passed_over), None, "{passed_over}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_tools_paths_are_what_they_read_changed_and_cd_into() {
        let tool = |id: &str, input: serde_json::Value| Row::new(id, 1, None, RowContent::Tool(ToolItem { item_id: id.into(), call: Some(ToolCall::new(id, "Bash", ToolKind::Other, "x", input)), ..Default::default() }));
        let rows = vec![
            tool("a", json!({ "file_path": "/w/a.md" })),
            tool("b", json!({ "command": "cd /w/repo && npm test; cd 'two words' && (cd ~/proj && ls)" })),
            tool("c", json!({ "path": "relative/x" })),
        ];
        assert_eq!(tool_paths(&rows, "/h"), ["/w/a.md", "/w/a.md", "/w/repo", "/h/proj"]);
        assert_eq!(cd_targets("cd \"/a b\"; echo cd /nope; abcd /x", "/h"), ["/a b"]);
        assert_eq!(cd_targets("cd ~", "/h"), ["/h"]);
    }

    #[test]
    fn another_macs_home_comes_from_its_folder() {
        assert_eq!(remote_home("/Users/mikeysee/dev/x", "/Users/me"), "/Users/mikeysee");
        assert_eq!(remote_home("/home/mike/x", "/Users/me"), "/Users/me");
    }
}
