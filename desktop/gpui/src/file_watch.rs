// Hearing folders change on this machine (`foundation.filewatch`), as the
// Swift app's DispatchSource watches do for the folder browser and an open
// file: notify's watcher (FSEvents on the Mac, inotify on Linux,
// ReadDirectoryChangesW on Windows), already in the build through gpui-kit.
// Only folders are watched, each on its own (not what's inside its folders):
// a file is heard through its folder, which also hears an agent's atomic write
// (a new file renamed over the old one) that a watch on the file would lose.
//
// What's heard is only a nudge: the folder or file is then read again through
// the runner, as everything the pane shows is.
use gpui_kit::*;
use notify::{RecursiveMode, Watcher as _};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A watch on some folders, telling its owner which of them changed.
pub(crate) struct FolderWatch {
    watcher: notify::RecommendedWatcher,
    /// Each folder watched, by its real path (what the system reports, after
    /// symlinks: /tmp is /private/tmp on the Mac) to the path it was asked for.
    folders: Arc<Mutex<HashMap<PathBuf, String>>>,
    _task: Task<()>,
}

impl FolderWatch {
    /// A watch that calls `changed` on its owner with the folders (as they
    /// were asked for) something happened in, or to, and the paths the system
    /// named (real ones), for a watch on one file in its folder to pick out.
    pub(crate) fn new<T: 'static>(cx: &mut Context<T>, changed: impl Fn(&mut T, Vec<String>, Vec<PathBuf>, &mut Context<T>) + 'static) -> Option<Self> {
        let folders: Arc<Mutex<HashMap<PathBuf, String>>> = Arc::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<PathBuf>>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                // Nothing to hear in a file being read.
                if !matches!(event.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(event.paths);
                }
            }
        })
        .ok()?;
        let lookup = folders.clone();
        let task = cx.spawn(async move |this, cx| {
            while let Some(paths) = rx.recv().await {
                let touched: Vec<String> = {
                    let Ok(folders) = lookup.lock() else { continue };
                    let mut out: Vec<String> = Vec::new();
                    for path in &paths {
                        // The folder itself, or the one it's in.
                        let found = folders.get(path).or_else(|| path.parent().and_then(|p| folders.get(p)));
                        if let Some(folder) = found {
                            if !out.contains(folder) {
                                out.push(folder.clone());
                            }
                        }
                    }
                    out
                };
                if touched.is_empty() {
                    continue;
                }
                if this.update(cx, |this, cx| changed(this, touched, paths, cx)).is_err() {
                    break;
                }
            }
        });
        Some(FolderWatch { watcher, folders, _task: task })
    }

    /// Watches exactly `wanted`: new ones start, ones no longer wanted stop.
    pub(crate) fn set(&mut self, wanted: &HashSet<String>) {
        let Ok(mut folders) = self.folders.lock() else { return };
        let gone: Vec<PathBuf> = folders.iter().filter(|(_, asked)| !wanted.contains(*asked)).map(|(real, _)| real.clone()).collect();
        for real in gone {
            let _ = self.watcher.unwatch(&real);
            folders.remove(&real);
        }
        for asked in wanted {
            if folders.values().any(|a| a == asked) {
                continue;
            }
            let watched = threadbear_core::open_policy::with_local_path(Path::new(asked), |local| {
                #[cfg(windows)]
                let real = local.to_path_buf();
                #[cfg(not(windows))]
                let real = std::fs::canonicalize(local)?;
                self.watcher.watch(&real, RecursiveMode::NonRecursive).map_err(std::io::Error::other)?;
                Ok(real)
            });
            if let Ok(real) = watched {
                folders.insert(real, asked.clone());
            }
        }
    }

    /// The folders watched now, as they were asked for.
    pub(crate) fn watching(&self) -> HashSet<String> {
        self.folders.lock().map(|f| f.values().cloned().collect()).unwrap_or_default()
    }
}
