// How the pane's files and folders show, remembered across launches: the Swift
// app keeps these in UserDefaults (`filesMarkdownView`, `folderBrowserMode`,
// `folderBrowserSort`, `folderBrowserSortAscending`,
// `folderBrowserShowHidden`); here a small file in the data folder, as the
// folded Highlights headings are (highlights_store.rs). Each is the same for
// every file and folder, as there.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use threadbear_client::config::RunnerConfig;
use threadbear_core::folders::{FolderMode, FolderSort, SortKey};
use threadbear_core::text_file::MarkdownView;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    markdown_view: Option<String>,
    folder_mode: Option<String>,
    folder_sort: Option<String>,
    folder_sort_ascending: Option<bool>,
    folder_show_hidden: Option<bool>,
}

/// What's remembered, read once when the pane is made.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FilePrefs {
    pub markdown_view: MarkdownView,
    pub folder_mode: FolderMode,
    pub folder_sort: FolderSort,
    pub show_hidden: bool,
}

fn path() -> PathBuf {
    RunnerConfig::from_env().home.join("desktop-files.json")
}

pub(crate) fn load() -> FilePrefs {
    let stored: Stored = std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let sort = match stored.folder_sort.as_deref().and_then(SortKey::from_name) {
        Some(key) => FolderSort { key, ascending: stored.folder_sort_ascending.unwrap_or(key == SortKey::Name) },
        None => FolderSort::default(),
    };
    FilePrefs {
        markdown_view: stored.markdown_view.as_deref().and_then(MarkdownView::from_name).unwrap_or_default(),
        folder_mode: stored.folder_mode.as_deref().and_then(FolderMode::from_name).unwrap_or_default(),
        folder_sort: sort,
        show_hidden: stored.folder_show_hidden.unwrap_or(false),
    }
}

pub(crate) fn save(prefs: &FilePrefs) {
    let stored = Stored {
        markdown_view: Some(prefs.markdown_view.name().to_owned()),
        folder_mode: Some(prefs.folder_mode.name().to_owned()),
        folder_sort: Some(prefs.folder_sort.key.name().to_owned()),
        folder_sort_ascending: Some(prefs.folder_sort.ascending),
        folder_show_hidden: Some(prefs.show_hidden),
    };
    if let Ok(text) = serde_json::to_string_pretty(&stored) {
        // Best effort: it's a preference, not data. Written whole, and only
        // this user's, as everything in the data folder is.
        let _ = threadbear_core::private_file::write(&path(), text.as_bytes());
    }
}
