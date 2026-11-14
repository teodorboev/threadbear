// The side pane's place in each thread, remembered across launches: the
// section it was showing (Highlights, Files, a panel) and the file or folder
// open over it. Without it a restart (the app updating itself several times a
// day) put every thread's pane back on the plain list, and Mike opened the
// same draft again after each one (13 Nov). A small file in the data folder,
// as the folded headings are (highlights_store.rs). A web page open in the
// pane isn't kept: it goes to the browser on opening (files.rs `begin_preview`).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use threadbear_client::config::RunnerConfig;

/// Threads kept, the most lately changed: an archived thread's place goes in time
/// rather than the file growing for ever.
const KEEP: usize = 300;

/// One thread's pane, as it was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Place {
    pub section: Option<String>,
    pub open: Option<Open>,
    /// When it last changed, in seconds since 1970, to know which to keep.
    pub at: u64,
}

/// The file or folder open over the section.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Open {
    pub path: String,
    pub folder: bool,
    pub title: String,
    /// The section it was opened from, which Back goes to.
    pub from: String,
    /// The folders and files Mike came through to it in the browser, which
    /// Back goes through first, last one nearest.
    pub back: Vec<Open>,
}

fn path() -> PathBuf {
    RunnerConfig::from_env().home.join("desktop-pane.json")
}

pub(crate) fn load() -> HashMap<String, Place> {
    load_from(&path())
}

pub(crate) fn save(places: &HashMap<String, Place>) {
    save_to(&path(), places)
}

fn load_from(path: &Path) -> HashMap<String, Place> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save_to(path: &Path, places: &HashMap<String, Place>) {
    let mut newest: Vec<(&String, &Place)> = places.iter().collect();
    newest.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| a.0.cmp(b.0)));
    let kept: HashMap<&String, &Place> = newest.into_iter().take(KEEP).collect();
    if let Ok(text) = serde_json::to_string(&kept) {
        // Best effort: it's where the pane was, not data. Written whole, and
        // only this user's, as everything in the data folder is: the paths
        // say what they've been reading.
        let _ = threadbear_core::private_file::write(path, text.as_bytes());
    }
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(at: u64, path: &str) -> Place {
        Place { section: Some("threadbear.highlights".into()), open: Some(Open { path: path.into(), folder: false, title: "post.md".into(), from: "threadbear.highlights".into(), back: vec![] }), at }
    }

    #[test]
    fn a_place_comes_back_as_it_was_left() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop-pane.json");
        let mut deep = place(7, "/blog/drafts/post.md");
        let folder = Open { path: "/blog/drafts".into(), folder: true, title: "drafts".into(), from: "threadbear.files".into(), back: vec![] };
        deep.open.as_mut().unwrap().back = vec![folder];
        let places = HashMap::from([("t1".to_owned(), place(5, "/blog/post.md")), ("t3".to_owned(), deep), ("t2".to_owned(), Place { section: Some("threadbear.files".into()), open: None, at: 6 })]);
        save_to(&file, &places);
        assert_eq!(load_from(&file), places);
    }

    #[test]
    fn only_the_latest_threads_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop-pane.json");
        let places: HashMap<String, Place> = (0..KEEP as u64 + 10).map(|i| (format!("t{i}"), place(i, "/a.md"))).collect();
        save_to(&file, &places);
        let back = load_from(&file);
        assert_eq!(back.len(), KEEP);
        assert!(!back.contains_key("t0") && back.contains_key(&format!("t{}", KEEP + 9)));
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop-pane.json");
        assert!(load_from(&file).is_empty());
        std::fs::write(&file, "not json").unwrap();
        assert!(load_from(&file).is_empty());
    }
}
