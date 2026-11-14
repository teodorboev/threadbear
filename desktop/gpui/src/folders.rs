// A folder in the side pane, a port of the Swift app's FolderBrowser: a list with
// columns and fold-out rows, or a grid of tiles with pictures' own thumbnails, a
// path bar to go back up (its "…" a menu of the folders it has no room for),
// right-click menus, more than one row picked at a time, and the Finder's keys
// (the arrows, Return, Space, ⌘↑, ⌘⇧., typing a name). What's in a folder
// comes from the runner's `files.list` (threadbear_core::folders has the rules),
// so a thread on another Mac browses the same; it's read again when the folder
// changes (file_watch.rs), every 15 seconds for another Mac's, and when the
// window comes to the front. Drag-out is later work (`drag.fileout`), and Space
// shows the file in the pane, as there's no Quick Look here (`file.quicklook`).
use crate::a11y::A11yRole as _;
use crate::call;
use crate::file_watch::FolderWatch;
use crate::highlights::{icon, palette, MenuItem, Palette, PanePreview, SidePane, Target, FILES};
use crate::style;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use threadbear_core::folders::{self as rules, crumbs, join, parent, size_words, Crumb, FolderContents, FolderEntry, FolderMode, FolderSort, Selection, SortKey};
use threadbear_core::highlights::is_app_project;
use threadbear_core::inbox::ThreadSummary;

const ROW_HEIGHT: f32 = 24.;
/// NSTableHeaderView's height on macOS 26 and 27, measured.
const HEADER_HEIGHT: f32 = 26.;
/// A row's own inset, and each level of folders in (NSOutlineView's, measured).
const ROW_INSET: f32 = 2.;
const LEVEL_INDENT: f32 = 13.;
const PATH_BAR_HEIGHT: f32 = 26.;
/// Narrower than this the Date Modified column goes, then Size
/// (FolderPaneView's layout).
const HIDE_DATE_UNDER: f32 = 380.;
const HIDE_SIZE_UNDER: f32 = 260.;
/// The path bar's arrow between folders, and its inset (`FolderPathBar`).
const PATH_ARROW: f32 = 14.;
const PATH_INSET: f32 = 8.;
/// A crumb's words are a borderless button's: its title and a little either side.
const CRUMB_PAD: f32 = 3.;
/// The gap between rows of tiles (`minimumLineSpacing`).
const TILE_GAP: f32 = 4.;
/// The key context of the folder browser, for its keys.
const CONTEXT: &str = "FolderBrowser";

/// One folder browser's state: its listings and what's open and picked in it.
pub(crate) struct FolderView {
    pub(crate) root: String,
    /// "bruce": a folder on that Mac. Nothing here can open or reveal its
    /// files, it has no thumbnails, and it's read again on a timer.
    away: Option<String>,
    listings: HashMap<String, FolderContents>,
    /// Folders asked for and not heard back about (`loadingFolders`).
    loading: HashSet<String>,
    expanded: HashSet<String>,
    selection: Selection,
    failure: Option<String>,
    sort: FolderSort,
    show_hidden: bool,
    mode: FolderMode,
    scroll: UniformListScrollHandle,
    grid_scroll: UniformListScrollHandle,
    /// Letters typed for type-select, and when the last came.
    typed: String,
    typed_at: Option<Instant>,
    /// Folders waiting for a burst of changes to settle before they're read.
    settling: HashSet<String>,
    /// The row a right-click menu is open for: it gets a ring while it's up.
    menu_row: Option<String>,
}

impl FolderView {
    fn new(root: String, away: Option<String>, sort: FolderSort, show_hidden: bool, mode: FolderMode) -> Self {
        Self {
            root,
            away,
            listings: HashMap::new(),
            loading: HashSet::new(),
            expanded: HashSet::new(),
            selection: Selection::default(),
            failure: None,
            sort,
            show_hidden,
            mode,
            scroll: UniformListScrollHandle::new(),
            grid_scroll: UniformListScrollHandle::new(),
            typed: String::new(),
            typed_at: None,
            settling: HashSet::new(),
            menu_row: None,
        }
    }

    fn entries_of(&self, folder: &str) -> Vec<FolderEntry> {
        let Some(listing) = self.listings.get(folder) else { return vec![] };
        let visible: Vec<FolderEntry> = listing.entries.iter().filter(|e| self.show_hidden || !e.hidden).cloned().collect();
        self.sort.apply(&visible)
    }

    /// The rows on screen, top to bottom, indented by depth.
    fn visible_rows(&self) -> Vec<(FolderEntry, usize)> {
        let mut out = Vec::new();
        fn add(view: &FolderView, folder: &str, depth: usize, out: &mut Vec<(FolderEntry, usize)>) {
            for entry in view.entries_of(folder) {
                let open = entry.is_folder && view.expanded.contains(&entry.path);
                out.push((entry.clone(), depth));
                if open {
                    add(view, &entry.path, depth + 1, out);
                }
            }
        }
        add(self, &self.root, 0, &mut out);
        out
    }

    /// What the keys and clicks move through: the list's rows, or the tiles.
    fn rows(&self) -> Vec<FolderEntry> {
        match self.mode {
            FolderMode::List => self.visible_rows().into_iter().map(|(e, _)| e).collect(),
            FolderMode::Icons => self.entries_of(&self.root),
        }
    }

    fn row_paths(&self) -> Vec<String> {
        self.rows().into_iter().map(|e| e.path).collect()
    }

    fn picked(&self) -> Vec<FolderEntry> {
        let rows = self.rows();
        rows.into_iter().filter(|e| self.selection.contains(&e.path)).collect()
    }

    /// The scripted app's and dump's line (`FolderPaneView.summary`).
    fn summary(&self) -> String {
        let rows = self.entries_of(&self.root);
        let names: Vec<String> = rows.iter().take(40).map(|e| e.name.clone()).collect();
        let drawn = match self.mode {
            FolderMode::List => self.visible_rows().len(),
            FolderMode::Icons => rows.len(),
        };
        let mut words = format!("({}, {} rows, sort {} {}, rows drawn {drawn}", self.mode.name(), rows.len(), self.sort.key.name(), if self.sort.ascending { "up" } else { "down" });
        let picked = self.picked();
        if !picked.is_empty() {
            words += &format!(", selected {}", picked.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(" + "));
        }
        if let Some(note) = self.note() {
            words += &format!(", says \"{note}\"");
        }
        words + "): " + &names.join(", ")
    }

    fn note(&self) -> Option<String> {
        if let Some(failure) = &self.failure {
            return Some(failure.clone());
        }
        if self.loading.contains(&self.root) && !self.listings.contains_key(&self.root) {
            return Some("Loading…".to_owned());
        }
        (self.listings.contains_key(&self.root) && self.entries_of(&self.root).is_empty()).then(|| rules::EMPTY_WORDS.to_owned())
    }

    /// The memory Back comes back to (`FolderMemory`).
    fn memory(&self) -> crate::highlights::FolderMemory {
        crate::highlights::FolderMemory { selected: self.selection.in_order(&self.row_paths()), expanded: self.expanded.clone() }
    }
}

fn core_folders_name(path: &str) -> &str {
    if threadbear_core::reducer::text::is_windows_path(path) { path.rsplit(['/', '\\']).next().unwrap_or(path) } else { path.rsplit('/').next().unwrap_or(path) }
}

/// Keeping a folder up to date while it shows: a watch on its folders here,
/// or a timer for another Mac's.
enum Keeping {
    Watch(FolderWatch),
    /// Held only so it runs: it stops when it's dropped.
    Poll { _timer: Task<()> },
}

/// A picture's thumbnail: on its way, made, or not to be had.
#[derive(Clone)]
enum Thumb {
    Making,
    Made(Arc<Image>),
    None,
}

/// Every open folder browser's state.
#[derive(Default)]
pub(crate) struct FoldersState {
    views: HashMap<String, FolderView>,
    /// The browser showing (its key), kept up to date.
    kept: Option<(String, Keeping)>,
    focus: Option<FocusHandle>,
    /// Pictures' thumbnails, by path, size and time, made once.
    thumbs: HashMap<String, Thumb>,
    /// Reads the folder showing again when the window comes back to the front:
    /// a file that grew changes nothing the watch hears.
    _activation: Option<Subscription>,
}

fn view_key(thread_id: &str, root: &str) -> String {
    format!("{thread_id}\n{root}")
}

/// `date_words`, the list's Date Modified column (`DateFormatter`, medium date,
/// short time, "Today" and "Yesterday" relative).
fn date_words(ms: i64) -> String {
    use threadbear_core::chrono::{Datelike as _, Local, TimeZone as _};
    let Some(date) = Local.timestamp_millis_opt(ms).single() else { return "--".to_owned() };
    let now = Local::now();
    let time = date.format("%-I:%M %p").to_string();
    let day_words = if date.date_naive() == now.date_naive() {
        "Today".to_owned()
    } else if date.date_naive().succ_opt() == Some(now.date_naive()) {
        "Yesterday".to_owned()
    } else if date.year() == now.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %Y").to_string()
    };
    format!("{day_words} at {time}")
}

// ---------------------------------------------------------------- icons

/// The Finder's blue folder, as an icon of the browser's: its tab, and the
/// front lighter, drawn in colour (an image, not a tinted symbol).
const FOLDER_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 84 84"><defs><linearGradient id="f" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#6BCBF7"/><stop offset="1" stop-color="#36ACE6"/></linearGradient></defs><path d="M3 15a4 4 0 0 1 4-4h20.5c1.3 0 2.4.5 3.2 1.4l3.3 3.6h43a4 4 0 0 1 4 4V69a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4z" fill="#2E9FDD"/><rect x="3" y="21" width="78" height="52" rx="4" fill="url(#f)"/></svg>"##;

/// A document's page, its corner turned down.
const DOCUMENT_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 84 84"><path d="M19.5 4.5h31l15 15v58a2 2 0 0 1-2 2h-44a2 2 0 0 1-2-2v-71a2 2 0 0 1 2-2z" fill="#FAFAFA" stroke="#C6C6C6"/><path d="M50.5 4.5v13a2 2 0 0 0 2 2h13" fill="#E4E4E4" stroke="#C6C6C6" stroke-linejoin="round"/></svg>"##;

fn folder_image() -> Arc<Image> {
    static MADE: OnceLock<Arc<Image>> = OnceLock::new();
    MADE.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Svg, FOLDER_SVG.to_vec()))).clone()
}

fn document_image() -> Arc<Image> {
    static MADE: OnceLock<Arc<Image>> = OnceLock::new();
    MADE.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Svg, DOCUMENT_SVG.to_vec()))).clone()
}

/// A picture's own thumbnail, made off the main thread from the file on this
/// machine, small (the tile's 84 points at 2x), never more than 8192 pixels a
/// side or 256 MB decoded, whatever the file says.
fn make_thumbnail(path: &str) -> Option<Arc<Image>> {
    use image::imageops::FilterType;
    let bytes = threadbear_core::open_policy::local_read(std::path::Path::new(path), 24 * 1024 * 1024).ok()?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    let small = picture.resize(168, 168, FilterType::Triangle).to_rgba8();
    let mut png = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

impl SidePane {
    /// The picture standing for `entry`: its thumbnail once it's made (asked
    /// for here, the first time), else the folder or the page.
    fn entry_image(&mut self, entry: &FolderEntry, away: bool, cx: &mut Context<Self>) -> Arc<Image> {
        if entry.is_folder {
            return folder_image();
        }
        if away || !rules::thumbnails(entry) {
            return document_image();
        }
        let key = format!("{}|{}|{}", entry.path, entry.size.unwrap_or(0), entry.modified_ms.unwrap_or(0));
        match self.folders.thumbs.get(&key) {
            Some(Thumb::Made(image)) => return image.clone(),
            Some(_) => return document_image(),
            None => {}
        }
        self.folders.thumbs.insert(key.clone(), Thumb::Making);
        let path = entry.path.clone();
        cx.spawn(async move |this, cx| {
            let made = cx.background_executor().spawn(async move { make_thumbnail(&path) }).await;
            this.update(cx, |this, cx| {
                this.folders.thumbs.insert(key, made.map_or(Thumb::None, Thumb::Made));
                cx.notify();
            })
            .ok();
        })
        .detach();
        document_image()
    }
}

// ---------------------------------------------------------------- reading

impl SidePane {
    /// The thread's Files panel: its folder, read again each time it's shown,
    /// so what the agent has made since is there.
    pub(crate) fn load_folder_for_files(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread(cx).filter(|t| t.id == thread_id) else { return };
        let key = view_key(thread_id, &thread.cwd);
        if self.folders.views.contains_key(&key) {
            self.refresh_folder(&key, cx);
        } else {
            self.ensure_folder(&thread, &thread.cwd, &thread.cwd, cx);
        }
    }

    pub(crate) fn load_folder_for_preview(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id).cloned() else { return };
        let Target::Folder(path) = &preview.target else { return };
        let key = view_key(&thread.id, path);
        // Opened again (a link clicked, Back): read it again.
        if self.folders.views.contains_key(&key) {
            self.refresh_folder(&key, cx);
        }
        self.ensure_folder(thread, path, path, cx);
    }

    /// Makes the folder view and asks for `folder`'s rows if it hasn't them.
    fn ensure_folder(&mut self, thread: &ThreadSummary, root: &str, folder: &str, cx: &mut Context<Self>) {
        let key = view_key(&thread.id, root);
        if !self.folders.views.contains_key(&key) {
            let away = thread.runner_id.as_deref().map(|id| self.app.upgrade().map_or_else(|| "another Mac".to_owned(), |a| a.read(cx).runner_name_of(id)));
            let prefs = self.files.prefs;
            let mut view = FolderView::new(root.to_owned(), away, prefs.folder_sort, prefs.show_hidden, prefs.folder_mode);
            // Back to a folder: what was picked and open there comes back.
            if let Some(memory) = self.previews.get(&thread.id).filter(|p| p.target == Target::Folder(root.to_owned())).map(|p| p.memory.clone()) {
                view.selection = Selection::of(memory.selected);
                view.expanded = memory.expanded;
            }
            self.folders.views.insert(key.clone(), view);
        }
        let Some(view) = self.folders.views.get(&key) else { return };
        if view.listings.contains_key(folder) || view.loading.contains(folder) {
            return;
        }
        let expanded: Vec<String> = view.expanded.iter().filter(|f| !view.listings.contains_key(*f)).cloned().collect();
        self.list_folder(&key, folder, cx);
        // The open rows Back brought back, read too.
        if folder == root {
            for open in expanded {
                self.list_folder(&key, &open, cx);
            }
        }
    }

    /// Asks the runner for `folder`'s rows, for the view `key`.
    fn list_folder(&mut self, key: &str, folder: &str, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get_mut(key) else { return };
        if !view.loading.insert(folder.to_owned()) {
            return;
        }
        let runner = self.thread(cx).filter(|t| key.starts_with(&format!("{}\n", t.id))).and_then(|t| t.runner_id);
        let mut params = json!({ "path": folder });
        if let Some(runner) = runner {
            params["runnerId"] = json!(runner);
        }
        let client = self.client.clone();
        let (key, folder) = (key.to_owned(), folder.to_owned());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = call(client, "files.list", params).await;
            this.update(cx, |this, cx| {
                let Some(view) = this.folders.views.get_mut(&key) else { return };
                view.loading.remove(&folder);
                match res {
                    Ok(v) => {
                        let mut contents = FolderContents::decode(&v);
                        // By the folder as it was asked for (`FolderListing.read`).
                        contents.path = folder.clone();
                        let listing = FolderContents { entries: contents.entries.iter().map(|e| FolderEntry { path: join(&folder, &e.name), ..e.clone() }).collect(), ..contents };
                        if folder == view.root {
                            view.failure = None;
                        }
                        // The same as before: nothing to draw again.
                        if view.listings.get(&folder) == Some(&listing) {
                            return;
                        }
                        view.listings.insert(folder.clone(), listing);
                        let rows = view.row_paths();
                        view.selection.keep_only(&rows);
                    }
                    Err(e) if folder == view.root => {
                        view.failure = Some(rules::failure_words(e.code, &e.message, view.away.as_deref()));
                        view.listings.remove(&folder);
                    }
                    // A folder open in the list that's gone closes; one that
                    // couldn't be read keeps what it had.
                    Err(e) if e.message.starts_with("No such folder") => {
                        view.listings.remove(&folder);
                        view.expanded.remove(&folder);
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reads the folder and every open row again.
    fn refresh_folder(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get(key) else { return };
        let mut folders: Vec<String> = view.listings.keys().cloned().collect();
        if !folders.contains(&view.root) {
            folders.push(view.root.clone());
        }
        for folder in folders {
            self.list_folder(key, &folder, cx);
        }
    }

    /// The watch heard these folders change: each is read again once its
    /// burst of changes settles, later for a huge one (`changed`).
    fn folders_changed(&mut self, key: &str, folders: Vec<String>, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get_mut(key) else { return };
        for folder in folders {
            if !view.listings.contains_key(&folder) && folder != view.root {
                continue;
            }
            if !view.settling.insert(folder.clone()) {
                continue;
            }
            let delay = rules::settle_ms(view.listings.get(&folder).map_or(0, |l| l.entries.len()));
            let key = key.to_owned();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(delay)).await;
                this.update(cx, |this, cx| {
                    if let Some(view) = this.folders.views.get_mut(&key) {
                        view.settling.remove(&folder);
                    }
                    this.list_folder(&key, &folder, cx);
                })
                .ok();
            })
            .detach();
        }
    }

    /// Keeps the folder showing up to date, and only that one: a watch on
    /// its folders here, a timer for another Mac's. Called as the pane draws.
    pub(crate) fn keep_folder_current(&mut self, cx: &mut Context<Self>) {
        let key = self.thread(cx).and_then(|t| self.current_folder_key(&t));
        let Some(key) = key else {
            self.folders.kept = None;
            return;
        };
        let Some(view) = self.folders.views.get(&key) else { return };
        let away = view.away.is_some();
        let mut wanted: HashSet<String> = view.listings.keys().cloned().collect();
        wanted.insert(view.root.clone());
        if self.folders.kept.as_ref().is_none_or(|(k, _)| *k != key) {
            let keeping = if away {
                let watched = key.clone();
                Keeping::Poll {
                    _timer: cx.spawn(async move |this, cx| loop {
                        cx.background_executor().timer(Duration::from_secs(rules::REMOTE_POLL_SECS)).await;
                        if this.update(cx, |this, cx| this.refresh_folder(&watched, cx)).is_err() {
                            break;
                        }
                    }),
                }
            } else {
                let watched = key.clone();
                match FolderWatch::new(cx, move |this: &mut SidePane, folders, _, cx| this.folders_changed(&watched, folders, cx)) {
                    Some(watch) => Keeping::Watch(watch),
                    None => {
                        self.folders.kept = None;
                        return;
                    }
                }
            };
            self.folders.kept = Some((key.clone(), keeping));
        }
        if let Some((_, Keeping::Watch(watch))) = &mut self.folders.kept {
            if watch.watching() != wanted {
                watch.set(&wanted);
            }
        }
    }
}

// ---------------------------------------------------------------- drawing

impl SidePane {
    pub(crate) fn render_files_panel(&mut self, thread: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let preview = crate::files::files_section_preview(thread);
        let root = thread.cwd.clone();
        self.render_folder(thread, &root, &preview, window, cx)
    }

    pub(crate) fn render_folder(&mut self, thread: &ThreadSummary, path: &str, preview: &PanePreview, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // Files panel's first draw, or a folder opened over a section.
        self.ensure_folder(thread, path, path, cx);
        if self.folders._activation.is_none() {
            self.folders._activation = Some(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    if let Some(key) = this.folders.kept.as_ref().map(|(k, _)| k.clone()) {
                        this.refresh_folder(&key, cx);
                    }
                }
            }));
        }
        let focus = self.folders.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let key = view_key(&thread.id, path);
        let Some(view) = self.folders.views.get(&key) else { return div().into_any_element() };
        let (note, mode, sort) = (view.note(), view.mode, view.sort);
        let crumb_list = crumbs(path, &self.folder_home(view.away.is_some(), path));
        let path_bar = self.path_bar(&key, &crumb_list, thread, preview, p, window, cx).into_any_element();
        let body = match (&note, mode) {
            (Some(note), _) => div().flex_1().min_h_0().child(crate::files::pane_note(note, p)).into_any_element(),
            (None, FolderMode::List) => self.render_list(&key, thread, preview, p, cx),
            (None, FolderMode::Icons) => self.render_grid(&key, thread, preview, p, cx),
        };
        let mut column = v_flex().size_full();
        if mode == FolderMode::List {
            column = column.child(self.folder_header(&key, sort, p, cx));
        }
        // The path bar under the rows, as the Finder's is.
        column = column.child(body).child(path_bar);
        let (keys_key, keys_thread, keys_preview) = (key.clone(), thread.clone(), preview.clone());
        div()
            .id("folder-browser")
            .size_full()
            .track_focus(&focus)
            .key_context(CONTEXT)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                if this.folder_key(&keys_key, ev, &keys_thread, &keys_preview, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(column)
            .into_any_element()
    }

    /// Another Mac's home, for its path bar: its folders under /Users say.
    fn folder_home(&self, away: bool, path: &str) -> String {
        if !away {
            return crate::files::home_dir();
        }
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 && parts[0] == "Users" {
            format!("/Users/{}", parts[1])
        } else {
            "/nowhere".to_owned()
        }
    }

    fn render_list(&mut self, key: &str, thread: &ThreadSummary, preview: &PanePreview, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.folders.views.get(key) else { return div().into_any_element() };
        let rows = view.visible_rows();
        let away = view.away.is_some();
        let selected: HashSet<String> = view.selection.paths.iter().cloned().collect();
        let ringed = view.menu_row.clone().filter(|_| self.menu.is_some());
        let expanded = view.expanded.clone();
        let scroll = view.scroll.clone();
        let images: Vec<Arc<Image>> = rows.iter().map(|(e, _)| e.clone()).collect::<Vec<_>>().iter().map(|e| self.entry_image(e, away, cx)).collect();
        let entity = cx.entity().downgrade();
        let (show_date, show_size) = (self.width >= HIDE_DATE_UNDER, self.width >= HIDE_SIZE_UNDER);
        let ring = style::control_accent(cx);
        let selected_fill = style::sidebar_selected(cx);
        let rows: std::rc::Rc<Vec<(FolderEntry, usize, Arc<Image>)>> = std::rc::Rc::new(rows.into_iter().zip(images).map(|((e, d), i)| (e, d, i)).collect());
        let count = rows.len();
        let (thread2, preview2, key2) = (thread.clone(), preview.clone(), key.to_owned());
        let list = uniform_list("folder-rows", count, move |range, _window, _cx| {
            let mut out = Vec::with_capacity(range.len());
            for index in range {
                let (entry, depth, image) = rows[index].clone();
                let row = FolderRow {
                    index,
                    is_open: entry.is_folder && expanded.contains(&entry.path),
                    selected: selected.contains(&entry.path),
                    ringed: ringed.as_deref() == Some(entry.path.as_str()),
                    entry,
                    depth,
                    image,
                };
                out.push(row.render(&entity, &key2, &thread2, &preview2, p, selected_fill, ring, show_date, show_size).into_any_element());
            }
            out
        })
        .track_scroll(&scroll)
        .size_full();
        self.folder_surface(key, thread, list.into_any_element(), cx)
    }

    /// Under the rows: a click on the blank space picks nothing, and its
    /// right-click is the empty space's menu.
    fn folder_surface(&mut self, key: &str, thread: &ThreadSummary, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let (empty_key, click_key) = (key.to_owned(), key.to_owned());
        let _ = thread;
        div()
            .id("folder-surface").a11y_role(Role::GenericContainer)
            .flex_1()
            .min_h_0()
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                // Only a click on the space itself: a row's own click has gone.
                if !ev.modifiers().secondary(cx) && !ev.modifiers().shift {
                    if let Some(view) = this.folders.views.get_mut(&click_key) {
                        view.selection = Selection::default();
                    }
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    if let Some(thread) = this.thread(cx) {
                        let items = this.folder_menu_items(&empty_key, None, &thread);
                        this.open_menu(items, ev.position, window, cx);
                    }
                }),
            )
            .child(content)
            .into_any_element()
    }

    /// The icon view: tiles in rows, as many to a row as fit, each the
    /// file's picture (a picture's own, here) with its name under it.
    fn render_grid(&mut self, key: &str, thread: &ThreadSummary, preview: &PanePreview, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.folders.views.get(key) else { return div().into_any_element() };
        let entries = view.entries_of(&view.root);
        let away = view.away.is_some();
        let selected: HashSet<String> = view.selection.paths.iter().cloned().collect();
        let ringed = view.menu_row.clone().filter(|_| self.menu.is_some());
        let scroll = view.grid_scroll.clone();
        let (columns, tile_width) = rules::grid_columns(self.width);
        let images: Vec<Arc<Image>> = entries.iter().map(|e| self.entry_image(e, away, cx)).collect();
        let tiles: std::rc::Rc<Vec<(FolderEntry, Arc<Image>)>> = std::rc::Rc::new(entries.into_iter().zip(images).collect());
        let row_count = tiles.len().div_ceil(columns.max(1));
        let entity = cx.entity().downgrade();
        let pick = style::control_accent(cx).opacity(0.45);
        let ring = style::control_accent(cx);
        let (thread2, preview2, key2) = (thread.clone(), preview.clone(), key.to_owned());
        let grid = uniform_list("folder-tiles", row_count, move |range, _window, _cx| {
            let mut out = Vec::with_capacity(range.len());
            for row in range {
                // Every row the same height, as the list measures its first
                // for all of them: the inset over the first is the list's own.
                let mut line = h_flex().px(px(rules::GRID_INSET)).pb(px(TILE_GAP));
                for index in row * columns..((row + 1) * columns).min(tiles.len()) {
                    let (entry, image) = tiles[index].clone();
                    let tile = FolderTile { index, selected: selected.contains(&entry.path), ringed: ringed.as_deref() == Some(entry.path.as_str()), entry, image, width: tile_width };
                    line = line.child(tile.render(&entity, &key2, &thread2, &preview2, p, pick, ring));
                }
                out.push(line.into_any_element());
            }
            out
        })
        .track_scroll(&scroll)
        .pt(px(rules::GRID_INSET))
        .size_full();
        self.folder_surface(key, thread, grid.into_any_element(), cx)
    }

    /// The bar under the rows: each folder up to this one, to click. A deep
    /// path keeps the nearest folders' names and puts the rest behind a "…"
    /// menu at the start, as many as it takes to fit (`FolderPathBar`).
    #[allow(clippy::too_many_arguments)]
    fn path_bar(&mut self, key: &str, crumb_list: &[Crumb], thread: &ThreadSummary, preview: &PanePreview, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = px(style::size::SMALL);
        let font = window.text_style().font();
        let widths: Vec<f32> = crumb_list
            .iter()
            .map(|c| {
                let run = TextRun { len: c.name.len(), font: font.clone(), color: p.fg, background_color: None, underline: None, strikethrough: None };
                f32::from(window.text_system().shape_line(c.name.clone().into(), size, &[run], None).width) / style::zoomed(1.) + CRUMB_PAD * 2.
            })
            .collect();
        let more = {
            let run = TextRun { len: "…".len(), font: font.clone(), color: p.fg, background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line("…".into(), size, &[run], None).width) / style::zoomed(1.) + CRUMB_PAD * 2.
        };
        let tucked = rules::tucked_count(&widths, PATH_ARROW, more + PATH_ARROW, self.width - PATH_INSET * 2.);
        let mut row = h_flex().flex_none().h(px(PATH_BAR_HEIGHT)).px(px(PATH_INSET)).items_center().border_t_1().border_color(p.hairline).text_size(size).text_color(p.secondary).overflow_x_hidden();
        let arrow = || div().flex_none().w(px(PATH_ARROW)).flex().items_center().justify_center().child(icon("chevron.right", style::size::SMALL - 3., p.faint));
        if tucked > 0 {
            // Nearest first, as it reads going up from where the menu is.
            let hidden: Vec<Crumb> = crumb_list[..tucked].iter().rev().cloned().collect();
            let below = crumb_list.get(tucked).map(|c| c.path.clone());
            let menu_key = key.to_owned();
            let _ = below;
            row = row.child(
                crate::highlights::tipped(div().id("crumb-more").flex_none().px(px(CRUMB_PAD)).rounded(px(4.)).cursor_pointer().hover(|s| s.text_color(p.fg)).child("…"), "The folders above these").on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    let items: Vec<MenuItem> = hidden
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            // Going up, the folder below it on the way here is picked.
                            let selecting = if i == 0 { None } else { Some(hidden[i - 1].path.clone()) };
                            MenuItem::action(c.name.clone(), "folder", crate::highlights::MenuAction::FolderCrumb { key: menu_key.clone(), path: c.path.clone(), selecting })
                        })
                        .collect();
                    this.open_menu(items, ev.position(), window, cx);
                })),
            );
            row = row.child(arrow());
        }
        let shown_count = crumb_list.len() - tucked;
        for (i, crumb) in crumb_list.iter().enumerate().skip(tucked) {
            let last = i + 1 == crumb_list.len();
            let (path, name) = (crumb.path.clone(), crumb.name.clone());
            let selecting = crumb_list.get(i + 1).map(|c| c.path.clone());
            let (key2, thread2, preview2) = (key.to_owned(), thread.clone(), preview.clone());
            let tip = if last { crumb.path.clone() } else { format!("Go to {}", crumb.path) };
            let words = div()
                .id(SharedString::from(format!("crumb-{i}"))).role(Role::Button)
                .px(px(CRUMB_PAD))
                .rounded(px(4.))
                // Only the last can still be too long, on its own: it truncates.
                .when(last, |s| s.min_w_0().truncate().text_color(p.fg))
                .when(!last, |s| s.flex_none().cursor_pointer().hover(|h| h.text_color(p.fg)))
                .child(name)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_to_crumb(&key2.clone(), path.clone(), selecting.clone(), &thread2, &preview2, cx)));
            row = row.child(crate::highlights::tipped(words, tip));
            if !last {
                row = row.child(arrow());
            }
        }
        let _ = shown_count;
        row
    }

    fn folder_header(&mut self, key: &str, sort: FolderSort, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        // NSTableHeaderView's look: the titles in semibold, the sort's arrow at
        // the end of the Name column, and a short rule between columns.
        let column = |this: &mut Self, sort_key: SortKey, title: &'static str, width: Option<f32>, right: bool, cx: &mut Context<Self>| {
            let active = sort.key == sort_key;
            let chevron = if !active { None } else { Some(if sort.ascending { "chevron.up" } else { "chevron.down" }) };
            let key2 = key.to_owned();
            let _ = &this;
            let tip = if active { "Sort the other way" } else { "Sort by this" };
            crate::highlights::tipped(
                div()
                    .id(SharedString::from(format!("sort-{}", sort_key.name())))
                    .relative()
                    // The rule before a column, in the gap, so the columns
                    // still line up with the rows' under them.
                    .when(width.is_some(), |s| s.child(div().absolute().left(px(-5.)).top(px(6.)).w(gpui_kit::px(1.)).h(px(14.)).bg(p.hairline)))
                    .when_some(width, |s, w| s.w(px(w)).flex_none())
                    .when(width.is_none(), |s| s.flex_1().min_w_0().justify_between().pr(px(6.)))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .when(right, |s| s.justify_end())
                    .cursor_pointer()
                    .text_color(p.fg)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(style::size::SMALL))
                    .child(title)
                    .when_some(chevron, |s, c| s.child(icon(c, 9., p.secondary))),
                tip,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                // Same column again turns it round; a new one starts its
                // useful way: names up, dates and sizes newest and biggest first.
                let ascending = if sort.key == sort_key { !sort.ascending } else { sort_key == SortKey::Name };
                this.set_folder_sort(&key2.clone(), FolderSort { key: sort_key, ascending }, cx);
            }))
        };
        let name = column(self, SortKey::Name, "Name", None, false, cx);
        let date = column(self, SortKey::Modified, "Date Modified", Some(150.), false, cx);
        let size = column(self, SortKey::Size, "Size", Some(72.), true, cx);
        let (show_date, show_size) = (self.width >= HIDE_DATE_UNDER, self.width >= HIDE_SIZE_UNDER);
        h_flex()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .pl(px(17.))
            .pr(px(10.))
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(p.hairline)
            .child(name)
            .when(show_date, |s| s.child(date))
            .when(show_size, |s| s.child(size))
    }
}

/// One row of the list.
struct FolderRow {
    index: usize,
    entry: FolderEntry,
    depth: usize,
    is_open: bool,
    selected: bool,
    ringed: bool,
    image: Arc<Image>,
}

impl FolderRow {
    #[allow(clippy::too_many_arguments)]
    fn render(self, entity: &WeakEntity<SidePane>, key: &str, thread: &ThreadSummary, preview: &PanePreview, p: Palette, selected_fill: Hsla, ring: Hsla, show_date: bool, show_size: bool) -> impl IntoElement {
        let FolderRow { index, entry, depth, is_open, selected, ringed, image } = self;
        let indent = ROW_INSET + depth as f32 * LEVEL_INDENT;
        let (click_entity, menu_entity, tri_entity) = (entity.clone(), entity.clone(), entity.clone());
        let (click_entry, menu_entry, tri_entry) = (entry.clone(), entry.clone(), entry.clone());
        let (click_key, menu_key, tri_key) = (key.to_owned(), key.to_owned(), key.to_owned());
        let (click_thread, menu_thread, tri_thread) = (thread.clone(), thread.clone(), thread.clone());
        let click_preview = preview.clone();
        let name_color = if entry.hidden { p.secondary } else { p.fg };
        // The triangle, then the icon a point after it, then the name 6 after
        // that, as NSOutlineView lays out its cell.
        let mut name_row = h_flex().flex_1().min_w_0().items_center();
        if entry.is_folder {
            name_row = name_row.child(
                crate::highlights::tipped(
                    div()
                        .id(("folder-tri", index))
                        .flex_none()
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        // NSOutlineView's triangle: as grey as a secondary label.
                        .child(icon(if is_open { "chevron.down" } else { "chevron.right" }, 10., p.secondary)),
                    if is_open { "Hide what's in it" } else { "Show what's in it" },
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    tri_entity.update(cx, |this, cx| this.toggle_expanded(&tri_key, &tri_entry, &tri_thread, cx)).ok();
                }),
            );
        } else {
            name_row = name_row.child(div().flex_none().w(px(14.)));
        }
        name_row = name_row
            // Hidden files look it, as they do in the Finder.
            .child(img(ImageSource::Image(image)).flex_none().ml(px(1.)).mr(px(6.)).size(px(18.)).object_fit(ObjectFit::Contain).when(entry.hidden, |s| s.opacity(0.55)))
            .child(div().flex_1().min_w_0().text_ellipsis_middle().whitespace_nowrap().overflow_hidden().text_size(px(style::size::LABEL)).text_color(name_color).child(entry.name.clone()));
        let date = entry.modified_ms.map(date_words).unwrap_or_else(|| "--".to_owned());
        let size = entry.size.map(size_words).unwrap_or_else(|| "--".to_owned());
        // Dragged out, it's the file or folder itself, on this Mac only
        // (file_drag.rs, `pasteboardWriterForItemAt`).
        let drag = Some(entry.path.clone()).filter(|_| !thread.is_remote());
        crate::file_drag::FileDragExt::file_drag(div().id(("folder-row", index)), drag)
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(indent))
            .pr(px(10.))
            .when(selected, |s| s.bg(selected_fill))
            .when(ringed, |s| s.border_2().border_color(ring).rounded(px(4.)))
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                let (mods, count) = (event.modifiers(), event.click_count());
                let toggle = mods.secondary(cx);
                let (entry, key, thread, preview) = (click_entry.clone(), click_key.clone(), click_thread.clone(), click_preview.clone());
                click_entity
                    .update(cx, |this, cx| {
                        if count >= 2 && !toggle && !mods.shift {
                            this.open_row(&key, &entry, &thread, &preview, cx);
                        } else {
                            this.click_row(&key, &entry.path, toggle, mods.shift, cx);
                        }
                    })
                    .ok();
                let _ = window;
            })
            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let (entry, key, thread, position) = (menu_entry.clone(), menu_key.clone(), menu_thread.clone(), event.position);
                menu_entity
                    .update(cx, |this, cx| {
                        let items = this.folder_menu_items(&key, Some(&entry), &thread);
                        if let Some(view) = this.folders.views.get_mut(&key) {
                            view.menu_row = Some(entry.path.clone());
                        }
                        this.open_menu(items, position, window, cx);
                    })
                    .ok();
            })
            .child(name_row)
            .when(show_date, |s| s.child(div().w(px(150.)).flex_none().truncate().text_size(px(style::size::DETAIL)).text_color(p.secondary).child(date)))
            .when(show_size, |s| s.child(div().w(px(72.)).flex_none().text_right().truncate().text_size(px(style::size::DETAIL)).text_color(p.secondary).child(size)))
    }
}

/// One tile of the icon view (`FolderGridItem`).
struct FolderTile {
    index: usize,
    entry: FolderEntry,
    selected: bool,
    ringed: bool,
    image: Arc<Image>,
    width: f32,
}

impl FolderTile {
    #[allow(clippy::too_many_arguments)]
    fn render(self, entity: &WeakEntity<SidePane>, key: &str, thread: &ThreadSummary, preview: &PanePreview, p: Palette, pick: Hsla, ring: Hsla) -> impl IntoElement {
        let FolderTile { index, entry, selected, ringed, image, width } = self;
        let (click_entity, menu_entity) = (entity.clone(), entity.clone());
        let (click_entry, menu_entry) = (entry.clone(), entry.clone());
        let (click_key, menu_key) = (key.to_owned(), key.to_owned());
        let (click_thread, menu_thread) = (thread.clone(), thread.clone());
        let click_preview = preview.clone();
        let drag = Some(entry.path.clone()).filter(|_| !thread.is_remote());
        crate::file_drag::FileDragExt::file_drag(div().id(("folder-tile", index)), drag)
            .flex_none()
            .w(px(width))
            .h(px(rules::TILE_HEIGHT))
            .relative()
            // Hidden files look it, as they do in the Finder.
            .when(entry.hidden, |s| s.opacity(0.55))
            .child(div().absolute().top_0().bottom_0().left(px(4.)).right(px(4.)).rounded(px(8.)).when(selected, |s| s.bg(pick)).when(ringed, |s| s.border_2().border_color(ring)))
            .child(
                v_flex()
                    .absolute()
                    .top(px(6.))
                    .left_0()
                    .right_0()
                    .items_center()
                    .gap(px(4.))
                    .child(img(ImageSource::Image(image)).flex_none().size(px(rules::TILE_PICTURE)).object_fit(ObjectFit::Contain))
                    .child(div().w_full().px(px(8.)).text_center().line_clamp(2).text_ellipsis_middle().text_size(px(style::size::SMALL)).line_height(px(style::natural_line(style::size::SMALL))).text_color(p.fg).child(entry.name.clone())),
            )
            .on_click(move |event, _, cx| {
                cx.stop_propagation();
                let (mods, count) = (event.modifiers(), event.click_count());
                let toggle = mods.secondary(cx);
                let (entry, key, thread, preview) = (click_entry.clone(), click_key.clone(), click_thread.clone(), click_preview.clone());
                click_entity
                    .update(cx, |this, cx| {
                        if count >= 2 && !toggle && !mods.shift {
                            this.open_row(&key, &entry, &thread, &preview, cx);
                        } else {
                            this.click_row(&key, &entry.path, toggle, mods.shift, cx);
                        }
                    })
                    .ok();
            })
            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let (entry, key, thread, position) = (menu_entry.clone(), menu_key.clone(), menu_thread.clone(), event.position);
                menu_entity
                    .update(cx, |this, cx| {
                        let items = this.folder_menu_items(&key, Some(&entry), &thread);
                        if let Some(view) = this.folders.views.get_mut(&key) {
                            view.menu_row = Some(entry.path.clone());
                        }
                        this.open_menu(items, position, window, cx);
                    })
                    .ok();
            })
    }
}

// ---------------------------------------------------------------- doing

impl SidePane {
    pub(crate) fn set_folder_sort(&mut self, key: &str, sort: FolderSort, cx: &mut Context<Self>) {
        self.files.prefs.folder_sort = sort;
        crate::files_store::save(&self.files.prefs);
        for view in self.folders.views.values_mut() {
            view.sort = sort;
        }
        let _ = key;
        cx.notify();
    }

    pub(crate) fn set_folder_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.files.prefs.show_hidden = hidden;
        crate::files_store::save(&self.files.prefs);
        for view in self.folders.views.values_mut() {
            view.show_hidden = hidden;
            let rows = view.row_paths();
            view.selection.keep_only(&rows);
        }
        cx.notify();
    }

    /// List or icons, for every folder, kept for the next (`FolderViewMode`):
    /// what's picked carries across, scrolled to.
    pub(crate) fn set_folder_mode(&mut self, mode: FolderMode, cx: &mut Context<Self>) {
        self.files.prefs.folder_mode = mode;
        crate::files_store::save(&self.files.prefs);
        for view in self.folders.views.values_mut() {
            view.mode = mode;
        }
        if let Some(key) = self.thread(cx).and_then(|t| self.current_folder_key(&t)) {
            self.reveal_selection(&key);
        }
        cx.notify();
    }

    fn toggle_expanded(&mut self, key: &str, entry: &FolderEntry, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let root = {
            let Some(view) = self.folders.views.get_mut(key) else { return };
            if !view.expanded.remove(&entry.path) {
                view.expanded.insert(entry.path.clone());
            } else {
                // It isn't showing, so it's not worth watching until it opens again.
                view.listings.remove(&entry.path);
                let rows = view.row_paths();
                view.selection.keep_only(&rows);
            }
            view.root.clone()
        };
        self.ensure_folder(thread, &root, &entry.path, cx);
        cx.notify();
    }

    fn select_row(&mut self, key: &str, entry: &FolderEntry, cx: &mut Context<Self>) {
        if let Some(view) = self.folders.views.get_mut(key) {
            let rows = view.row_paths();
            view.selection.click(&rows, &entry.path, false, false);
        }
        cx.notify();
    }

    /// A click on a row or tile, with ⌘ (`toggle`) or ⇧ (`extend`) held.
    fn click_row(&mut self, key: &str, path: &str, toggle: bool, extend: bool, cx: &mut Context<Self>) {
        if let Some(view) = self.folders.views.get_mut(key) {
            let rows = view.row_paths();
            view.selection.click(&rows, path, toggle, extend);
        }
        cx.notify();
    }

    /// The first picked row scrolled into sight.
    fn reveal_selection(&mut self, key: &str) {
        let Some(view) = self.folders.views.get(key) else { return };
        let rows = view.row_paths();
        let Some(index) = rows.iter().position(|r| view.selection.contains(r)) else { return };
        match view.mode {
            FolderMode::List => view.scroll.scroll_to_item(index, ScrollStrategy::Nearest),
            FolderMode::Icons => view.grid_scroll.scroll_to_item(index / rules::grid_columns(self.width).0.max(1), ScrollStrategy::Nearest),
        }
    }

    /// A double-click on a row: a folder goes in, a file shows in the pane
    /// with Back to here, a project file opens in its app (`openFromFolder`).
    fn open_row(&mut self, key: &str, entry: &FolderEntry, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let mut memory = self.folders.views.get(key).map(|v| v.memory()).unwrap_or_default();
        // Leaving a folder for a file, a folder or an app, it's the row
        // opened that's picked when Mike comes back.
        memory.selected = vec![entry.path.clone()];
        self.leave_folder(key, entry.path.clone(), entry.name.clone(), entry.is_folder, memory, thread, preview, cx);
    }

    /// Goes from the folder showing to `path`: in the pane, with Back to here
    /// as `memory` left it.
    #[allow(clippy::too_many_arguments)]
    fn leave_folder(&mut self, key: &str, path: String, name: String, is_folder: bool, memory: crate::highlights::FolderMemory, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let _ = key;
        let iwork_package = cfg!(target_os = "macos") && !thread.is_remote() && std::path::Path::new(&path).extension().is_some_and(|e| matches!(e.to_string_lossy().to_ascii_lowercase().as_str(), "key" | "pages" | "numbers"));
        let mut current = preview.clone();
        current.memory = memory;
        let back_stack = {
            let mut back = std::mem::take(&mut current.back);
            back.push(current.clone());
            back
        };
        if is_folder && !iwork_package {
            let mut next = PanePreview::new(Target::Folder(path), name, preview.from.clone());
            next.back = back_stack;
            self.show_preview(next, &thread.id, cx);
            return;
        }
        if !thread.is_remote() && is_app_project(&path) {
            self.open_externally(&path, cx);
            return;
        }
        let mut next = PanePreview::new(Target::File(path), name, preview.from.clone());
        next.back = back_stack;
        self.show_preview(next, &thread.id, cx);
    }

    /// A folder in the path bar, or its "…" menu: there, with Back to here,
    /// and the folder below it on the way here picked, as the Finder does.
    fn go_to_crumb(&mut self, key: &str, path: String, selecting: Option<String>, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let root_is = self.folders.views.get(key).map(|v| v.root.clone());
        if root_is.as_deref() == Some(path.as_str()) {
            return;
        }
        let memory = self.folders.views.get(key).map(|v| v.memory()).unwrap_or_default();
        let name = core_folders_name(&path).to_owned();
        self.leave_folder(key, path.clone(), name, true, memory, thread, preview, cx);
        if let Some(selecting) = selecting {
            // A Windows parent/breadcrumb uses forward slashes, while the
            // folder we came out of may still have its runner's backslashes.
            // Rows use the destination listing's spelling too.
            let selecting = join(&path, core_folders_name(&selecting));
            if let Some(view) = self.folders.views.get_mut(&view_key(&thread.id, &path)) {
                view.selection = Selection::of(vec![selecting]);
            } else if let Some(p) = self.previews.get_mut(&thread.id) {
                p.memory.selected = vec![selecting];
            }
        }
    }

    /// `⌘↑` (`goUp`): the folder above, with the one Mike came up out of picked.
    pub(crate) fn folder_up(&mut self, key: &str, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let Some(root) = self.folders.views.get(key).map(|v| v.root.clone()) else { return };
        let Some(up) = parent(&root) else { return };
        self.go_to_crumb(key, up, Some(root), thread, preview, cx);
    }

    /// Return, Enter, ⌘↓, and Space here: the first row picked, opened.
    fn open_picked(&mut self, key: &str, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let Some(first) = self.folders.views.get(key).and_then(|v| v.picked().into_iter().next()) else { return };
        self.open_row(key, &first, thread, preview, cx);
    }

    /// The Finder's keys a list or the icons take. True when it's used.
    fn folder_key(&mut self, key: &str, ev: &KeyDownEvent, thread: &ThreadSummary, preview: &PanePreview, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let stroke = &ev.keystroke;
        let mods = stroke.modifiers;
        let command = mods.secondary(cx);
        let plain = !command && !mods.alt && !mods.control && !mods.shift;
        let _ = window;
        let Some(view) = self.folders.views.get(key) else { return false };
        let mode = view.mode;
        match stroke.key.as_str() {
            // Space: Quick Look's job in the Swift app; here the file shows in
            // the pane, as another Mac's does there.
            "space" if plain => self.open_picked(key, thread, preview, cx),
            "enter" if plain || command => self.open_picked(key, thread, preview, cx),
            "down" if command && !mods.shift => self.open_picked(key, thread, preview, cx),
            "up" if command && !mods.shift => self.folder_up(key, thread, preview, cx),
            // ⌘⇧. (some keyboards name the key by its shifted ">").
            "." | ">" if command && mods.shift => {
                let hidden = !self.files.prefs.show_hidden;
                self.set_folder_hidden(hidden, cx);
            }
            "a" if command => {
                if let Some(view) = self.folders.views.get_mut(key) {
                    let rows = view.row_paths();
                    view.selection.select_all(&rows);
                }
                cx.notify();
            }
            "c" if command => self.copy_picked_paths(key, cx),
            "up" | "down" if !command && !mods.alt && !mods.control => {
                let columns = if mode == FolderMode::Icons { rules::grid_columns(self.width).0 as isize } else { 1 };
                let by = if stroke.key == "up" { -columns } else { columns };
                self.step_selection(key, by, mods.shift, cx);
            }
            "left" | "right" if mode == FolderMode::Icons && !command && !mods.alt && !mods.control => {
                self.step_selection(key, if stroke.key == "left" { -1 } else { 1 }, mods.shift, cx);
            }
            // The list's triangles: → opens a folder, ← closes it, or goes
            // to the folder it's in.
            "right" | "left" if mode == FolderMode::List && plain => self.fold_picked(key, stroke.key == "right", thread, cx),
            _ => {
                // Typing a name picks its row.
                let typed = stroke.key_char.clone().filter(|c| !command && !mods.control && c.chars().all(|ch| !ch.is_control()) && !c.is_empty());
                let Some(typed) = typed else { return false };
                self.type_select(key, &typed, cx);
            }
        }
        true
    }

    fn step_selection(&mut self, key: &str, by: isize, extend: bool, cx: &mut Context<Self>) {
        if let Some(view) = self.folders.views.get_mut(key) {
            let rows = view.row_paths();
            view.selection.step(&rows, by, extend);
        }
        self.reveal_selection(key);
        cx.notify();
    }

    /// → and ← on a picked row of the list: a folder opens or closes; ← on a
    /// row inside an open folder goes to that folder.
    fn fold_picked(&mut self, key: &str, open: bool, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get(key) else { return };
        let Some(entry) = view.picked().into_iter().next() else { return };
        let is_open = view.expanded.contains(&entry.path);
        if entry.is_folder && open != is_open {
            self.toggle_expanded(key, &entry, thread, cx);
            return;
        }
        if !open {
            let up = parent(&entry.path).filter(|p| view.expanded.contains(p));
            if let Some(up) = up {
                if let Some(view) = self.folders.views.get_mut(key) {
                    let rows = view.row_paths();
                    view.selection.click(&rows, &up, false, false);
                }
                self.reveal_selection(key);
                cx.notify();
            }
        }
    }

    /// Letters typed quickly pick the first row whose name starts with them;
    /// a pause starts again (NSTableView's type select).
    fn type_select(&mut self, key: &str, letters: &str, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get_mut(key) else { return };
        let now = Instant::now();
        if view.typed_at.is_none_or(|at| now.duration_since(at) > Duration::from_millis(rules::TYPE_SELECT_PAUSE_MS)) {
            view.typed.clear();
        }
        view.typed.push_str(letters);
        view.typed_at = Some(now);
        let rows = view.rows();
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        let picked = rows.iter().position(|e| view.selection.contains(&e.path));
        if let Some(index) = rules::type_select(&names, &view.typed, picked) {
            let paths: Vec<String> = rows.iter().map(|e| e.path.clone()).collect();
            view.selection.click(&paths, &rows[index].path, false, false);
            self.reveal_selection(key);
        }
        cx.notify();
    }

    /// ⌘C: the picked rows' paths, a line each.
    fn copy_picked_paths(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(view) = self.folders.views.get(key) else { return };
        let paths: Vec<String> = view.picked().into_iter().map(|e| e.path).collect();
        if !paths.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
        }
    }

    /// `FolderPaneView.menu`: a row's menu, for the rows picked if it's one of
    /// them, or the empty space's.
    pub(crate) fn folder_menu_items(&self, key: &str, entry: Option<&FolderEntry>, thread: &ThreadSummary) -> Vec<MenuItem> {
        use crate::highlights::MenuAction as A;
        let mut items = vec![];
        let view = self.folders.views.get(key);
        if let Some(entry) = entry {
            let picked = view.map(|v| v.picked()).unwrap_or_default();
            let rows: Vec<FolderEntry> = if picked.iter().any(|e| e.path == entry.path) { picked } else { vec![entry.clone()] };
            let paths: Vec<String> = rows.iter().map(|e| e.path.clone()).collect();
            items.push(MenuItem::action("Open", if entry.is_folder { "folder" } else { "sidebar.right" }, A::OpenFolderRow { key: key.to_owned(), entry: entry.clone() }));
            if !thread.is_remote() {
                items.push(MenuItem::action(if entry.is_folder { "Open in Finder" } else { "Open in its app" }, "arrow.up.forward.app", A::OpenEach { paths: paths.clone() }));
                items.push(MenuItem::action(crate::files::REVEAL_TITLE, "folder", A::RevealEach { paths: paths.clone() }));
            }
            items.push(MenuItem::action(if paths.len() > 1 { "Copy paths" } else { "Copy path" }, "doc.on.doc", A::Copy { text: paths.join("\n") }));
            items.push(MenuItem::separator());
        }
        let prefs = self.files.prefs;
        let (hidden, sort, mode) = (prefs.show_hidden, prefs.folder_sort, prefs.folder_mode);
        let mut sort_children = vec![];
        for key_kind in [SortKey::Name, SortKey::Modified, SortKey::Size] {
            let symbol = match key_kind {
                SortKey::Name => "textformat",
                SortKey::Modified => "calendar",
                SortKey::Size => "scalemass",
            };
            let ascending = if sort.key == key_kind { !sort.ascending } else { key_kind == SortKey::Name };
            let mut item = MenuItem::action(key_kind.title(), symbol, A::SortFolder { key: key.to_owned(), sort: FolderSort { key: key_kind, ascending } });
            item.checked = sort.key == key_kind;
            sort_children.push(item);
        }
        // The bar's button too, in the same words, where a right-click finds it.
        items.push(MenuItem::action(mode.switch_title(), mode.switch_symbol(), A::FolderMode { mode: mode.other() }));
        items.push(MenuItem::submenu("Sort by", "arrow.up.arrow.down", sort_children));
        items.push(MenuItem::action(if hidden { "Hide hidden files" } else { "Show hidden files" }, if hidden { "eye.slash" } else { "eye" }, A::FolderHidden { hidden: !hidden }));
        items
    }

    pub(crate) fn run_folder_row_action(&mut self, key: &str, entry: FolderEntry, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let preview = self.folder_preview(thread);
        self.open_row(key, &entry, thread, &preview, cx);
    }

    /// The path bar's "…" menu, a folder picked.
    pub(crate) fn run_folder_crumb_action(&mut self, key: &str, path: String, selecting: Option<String>, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let preview = self.folder_preview(thread);
        self.go_to_crumb(key, path, selecting, thread, &preview, cx);
    }

    /// The preview the folder showing belongs to: the pane's own, or the
    /// Files panel's.
    fn folder_preview(&self, thread: &ThreadSummary) -> PanePreview {
        self.previews.get(&thread.id).filter(|p| matches!(p.target, Target::Folder(_))).cloned().unwrap_or_else(|| crate::files::files_section_preview(thread))
    }

    /// The bar's button over a folder: the other view (`FolderModeButton`).
    pub(crate) fn folder_mode_button(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.files.prefs.folder_mode;
        self.header_button("hl-folder-mode", mode.switch_symbol(), mode.switch_title().to_owned(), p, cx, move |this, cx| this.set_folder_mode(mode.other(), cx))
    }

    #[cfg(feature = "automation")]
    pub(crate) fn folder_summary(&self, thread: &ThreadSummary, cx: &App) -> Option<String> {
        let _ = cx;
        let key = self.current_folder_key(thread)?;
        self.folders.views.get(&key).map(|v| v.summary())
    }

    /// The folder browser showing, if any: the pane's own or the Files panel's.
    pub(crate) fn current_folder_key(&self, thread: &ThreadSummary) -> Option<String> {
        let section = self.section_for(thread);
        if let Some(preview) = self.previews.get(&thread.id).filter(|p| p.from == section) {
            return match &preview.target {
                Target::Folder(path) => Some(view_key(&thread.id, path)),
                _ => None,
            };
        }
        (section == FILES).then(|| view_key(&thread.id, &thread.cwd))
    }

    /// What the path bar shows and has behind its "…", as the Swift
    /// automation reads them: the tucked folders nearest first.
    #[cfg(feature = "automation")]
    fn path_bar_words(&self, key: &str, window: &mut Window) -> (String, Vec<String>) {
        let Some(view) = self.folders.views.get(key) else { return (String::new(), vec![]) };
        let list = crumbs(&view.root, &self.folder_home(view.away.is_some(), &view.root));
        let size = px(style::size::SMALL);
        let font = window.text_style().font();
        let width = |text: &str, window: &mut Window| {
            let run = TextRun { len: text.len(), font: font.clone(), color: gpui_kit::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(text.to_owned().into(), size, &[run], None).width) / style::zoomed(1.) + CRUMB_PAD * 2.
        };
        let widths: Vec<f32> = list.iter().map(|c| width(&c.name, window)).collect();
        let more = width("…", window);
        let tucked = rules::tucked_count(&widths, PATH_ARROW, more + PATH_ARROW, self.width - PATH_INSET * 2.);
        let shown: Vec<String> = list[tucked..].iter().map(|c| c.name.clone()).collect();
        let words = if tucked > 0 { format!("… > {}", shown.join(" > ")) } else { shown.join(" > ") };
        (words, list[..tucked].iter().rev().map(|c| c.name.clone()).collect())
    }
}

/// The `folder` automation command (`Automation.swift`): "open" is a
/// double-click on that row, "select" a click, "selectAlso" a ⌘-click,
/// "expand" its triangle, "up" ⌘↑, "mode" the bar's button, "sort" a column,
/// "hidden" ⌘⇧., "quickLook" what Space would show, "click" a press on a row
/// ("count" 2 for a double-click), "key" a key by its Mac key code
/// ("command", "shift", "characters"), "crumb" a path bar folder, "pathBar"
/// its words, "pathMenu" what's behind its "…", "menu" a row's right-click
/// menu, "choose" one of its items.
#[cfg(feature = "automation")]
pub(crate) fn folder_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let pane = side.read(cx);
    let key = pane.current_folder_key(thread).ok_or("no folder is showing in the pane")?;
    let preview = pane.folder_preview(thread);
    let find = |pane: &SidePane, name: &str| pane.folders.views.get(&key).and_then(|v| v.rows().into_iter().find(|e| e.name == name));
    if let Some(mode) = cmd["mode"].as_str() {
        let mode = FolderMode::from_name(mode).ok_or("mode is list or icons")?;
        side.update(cx, |pane, cx| pane.set_folder_mode(mode, cx));
    }
    if let Some(sort) = cmd["sort"].as_str() {
        let sort_key = SortKey::from_name(sort).ok_or("sort is name, modified or size")?;
        let ascending = cmd["ascending"].as_bool().unwrap_or(true);
        side.update(cx, |pane, cx| pane.set_folder_sort(&key, FolderSort { key: sort_key, ascending }, cx));
    }
    if let Some(hidden) = cmd["hidden"].as_bool() {
        side.update(cx, |pane, cx| pane.set_folder_hidden(hidden, cx));
    }
    if let Some(name) = cmd["expand"].as_str() {
        let entry = find(side.read(cx), name).filter(|e| e.is_folder).ok_or_else(|| format!("no folder called {name}"))?;
        side.update(cx, |pane, cx| pane.toggle_expanded(&key, &entry, thread, cx));
    }
    if let Some(name) = cmd["select"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        side.update(cx, |pane, cx| pane.select_row(&key, &entry, cx));
    }
    if let Some(name) = cmd["selectAlso"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        side.update(cx, |pane, cx| pane.click_row(&key, &entry.path, true, false, cx));
    }
    if cmd["quickLook"].as_bool() == Some(true) {
        let pane = side.read(cx);
        let view = pane.folders.views.get(&key).ok_or("no folder is showing")?;
        let paths: Vec<String> = if view.away.is_some() { vec![] } else { view.picked().into_iter().map(|e| e.path).collect() };
        return Ok(json!(format!("ok would look at {}", serde_json::to_string(&paths).unwrap_or_default())));
    }
    if let Some(name) = cmd["open"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        side.update(cx, |pane, cx| pane.open_row(&key, &entry, thread, &preview, cx));
    }
    if let Some(name) = cmd["click"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        let count = cmd["count"].as_u64().unwrap_or(1);
        let (toggle, extend) = (cmd["command"].as_bool() == Some(true), cmd["shift"].as_bool() == Some(true));
        side.update(cx, |pane, cx| {
            if count >= 2 {
                pane.open_row(&key, &entry, thread, &preview, cx);
            } else {
                pane.click_row(&key, &entry.path, toggle, extend, cx);
            }
        });
    }
    if let Some(code) = cmd["key"].as_u64() {
        // The Mac's key codes the Swift app's script sends, as GPUI names them.
        let named = match code {
            49 => "space",
            36 | 76 => "enter",
            125 => "down",
            126 => "up",
            123 => "left",
            124 => "right",
            _ => "",
        };
        let characters = cmd["characters"].as_str().unwrap_or("").to_owned();
        let key_name = if named.is_empty() { characters.to_lowercase() } else { named.to_owned() };
        let mut modifiers = Modifiers::default();
        if cmd["command"].as_bool() == Some(true) {
            if cfg!(target_os = "macos") {
                modifiers.platform = true;
            } else {
                modifiers.control = true;
            }
        }
        modifiers.shift = cmd["shift"].as_bool() == Some(true);
        let key_char = (named.is_empty() && !characters.is_empty()).then_some(characters);
        let event = KeyDownEvent { keystroke: Keystroke { modifiers, key: key_name, key_char }, is_held: false, prefer_character_input: false };
        side.update(cx, |pane, cx| {
            pane.folder_key(&key, &event, thread, &preview, window, cx);
        });
    }
    if let Some(name) = cmd["crumb"].as_str() {
        let (path, selecting) = {
            let pane = side.read(cx);
            let view = pane.folders.views.get(&key).ok_or("no folder is showing")?;
            let list = crumbs(&view.root, &pane.folder_home(view.away.is_some(), &view.root));
            let index = list.iter().position(|c| c.name == name).ok_or_else(|| format!("no folder called {name} in the path bar"))?;
            (list[index].path.clone(), list.get(index + 1).map(|c| c.path.clone()))
        };
        side.update(cx, |pane, cx| pane.go_to_crumb(&key, path, selecting, thread, &preview, cx));
    }
    if cmd["pathBar"].as_bool() == Some(true) {
        let (words, _) = side.read(cx).path_bar_words(&key, window);
        return Ok(json!(format!("ok {words}")));
    }
    if cmd["pathMenu"].as_bool() == Some(true) {
        let (_, tucked) = side.read(cx).path_bar_words(&key, window);
        return Ok(json!(format!("ok {}", tucked.join(" | "))));
    }
    if let Some(title) = cmd["choose"].as_str() {
        let row = cmd["row"].as_str();
        let action = {
            let pane = side.read(cx);
            let entry = match row {
                Some(name) if !name.is_empty() => Some(find(pane, name).ok_or_else(|| format!("no row called {name}"))?),
                _ => None,
            };
            let items = pane.folder_menu_items(&key, entry.as_ref(), thread);
            let all = flatten(items);
            let item = all.into_iter().find(|i| i.title == title).ok_or_else(|| format!("no menu item {title}"))?;
            item.action
        };
        if let Some(action) = action {
            side.update(cx, |pane, cx| pane.run_menu_action(action, thread, window, cx));
        }
    }
    if let Some(name) = cmd["menu"].as_str() {
        let pane = side.read(cx);
        let entry = if name.is_empty() { None } else { Some(find(pane, name).ok_or_else(|| format!("no row called {name}"))?) };
        let items = pane.folder_menu_items(&key, entry.as_ref(), thread);
        let titles: Vec<String> = items.iter().map(|i| i.title.clone()).collect();
        return Ok(json!(format!("ok {}", titles.join(" | "))));
    }
    if cmd["up"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.folder_up(&key, thread, &preview, cx));
    }
    let pane = side.read(cx);
    let preview_words = pane.previews.get(&thread.id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
    let summary = pane.current_folder_key(thread).and_then(|k| pane.folders.views.get(&k)).map_or_else(|| "no folder".to_owned(), |v| v.summary());
    Ok(json!(format!("ok {preview_words}; {summary}")))
}

#[cfg(feature = "automation")]
fn flatten(items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut out = vec![];
    for item in items {
        let children = item.children.clone();
        out.push(item);
        out.extend(flatten(children));
    }
    out
}
