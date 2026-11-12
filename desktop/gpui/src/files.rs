// A file or page open in the pane, in place of its section: a port of the Swift
// app's PreviewBody, QuickLookView/FilePreviewView and TextFileView. What the
// file holds always comes from the runner (`files.read`), so a thread on
// another Mac works the same. A Markdown file's own text (its Markdown view)
// is the one thing read here, from a thread on this runner (file_edit.rs).
//
// A picture is drawn here with Copy Image; Markdown drawn as a page, or its
// text with the bar's Preview/Markdown menu; code and plain text in the kit's
// read-only editor, coloured by its highlighter; any text with the find bar
// over it (file_find.rs). On a Mac a video plays and a PDF shows in the
// system's own view over the pane (media_view.rs), read from this Mac's own
// file once the open gate has checked it; on Windows and Linux, and for
// everything else, the pane says it opens in its app (`file.quicklook`), and
// web pages go to the browser (`file.htmlpreview`, `panel.web` are later work).
use crate::file_edit::{FileEdit, SaveFile};
use crate::file_find::{FindBar, FindNext, FindPrevious};
use crate::file_watch::FolderWatch;
use crate::files_store::FilePrefs;
use crate::highlights::{palette, Palette, PanePreview, SidePane, Target, FILES};
use crate::{call, icons, style};
use gpui_kit::base::{TestSupportExt as _, TextViewState, TextViewStyle};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, spinner::Spinner, v_flex, ActiveTheme as _, Selectable, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use threadbear_core::folders::{picture_size, FileRead};
use threadbear_core::highlights::{self as core, Viewer};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::text_file::{words, MarkdownView, LIMIT};

/// Whether the system has "Finder", for the menu's words.
#[cfg(target_os = "macos")]
pub(crate) const REVEAL_TITLE: &str = "Reveal in Finder";
#[cfg(not(target_os = "macos"))]
pub(crate) const REVEAL_TITLE: &str = "Show in folder";

pub(crate) const OPEN_IN_BROWSER: &str = "Open in Browser";

/// The bytes of a file come from the runner; this is how much a preview asks for
/// (`files.read`'s own default is 16 MB, its limit 48).
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Past this much text, Markdown and code are shown plain: highlighting
/// a huge file line by line would stall (`TextFileController.highlightLimit`).
const HIGHLIGHT_LIMIT: usize = 400_000;

/// A line of code in the pane: the Swift text view's for its 12 point
/// monospaced font (measured, 15 apart).
pub(crate) const CODE_LINE: f32 = 15.;

/// What the pane's reading: one open at a time per thread.
pub(crate) struct OpenFile {
    pub path: String,
    pub opened: u64,
    pub body: FileBody,
    /// Only the start of the file came (`truncated`).
    pub truncated: bool,
    /// The whole file's size, as the runner said: past `LIMIT`, it's never
    /// shown as its raw text to work on (`TextFileView.canEdit`).
    pub size: i64,
    /// The find bar, while it's up (⌘F).
    pub find: Option<FindBar>,
    /// The file's own text being worked on: the pencil, or a Markdown file's
    /// Markdown view.
    pub edit: Option<FileEdit>,
    /// The drawn Markdown page's scroll, which the find bar moves to a match.
    pub page_scroll: ScrollHandle,
    /// Hears the drawn page change (its parse landing) for the find bar.
    _page_watch: Option<Subscription>,
    /// The file's folder on this machine, watched so an agent's rewrite shows
    /// up by itself while it's being read (`TextFileDocument.watch`).
    _file_watch: Option<FolderWatch>,
    /// The read that follows a change, once its burst settles.
    _reread: Option<Task<()>>,
}

impl OpenFile {
    fn new(path: String, opened: u64, body: FileBody) -> Self {
        OpenFile { path, opened, body, truncated: false, size: 0, find: None, edit: None, page_scroll: ScrollHandle::new(), _page_watch: None, _file_watch: None, _reread: None }
    }

    /// The raw text's editor, while it's showing.
    pub(crate) fn raw_editor(&self) -> Option<Entity<EditorState>> {
        self.edit.as_ref().map(|e| e.editor.clone())
    }

    /// Text this pane shows that can be worked on as text: Markdown or code
    /// that came whole and isn't too big (`TextFileView.shows`, `canEdit`).
    fn is_whole_text(&self) -> bool {
        matches!(self.body, FileBody::Markdown { .. } | FileBody::Text { .. }) && !self.truncated && self.size < LIMIT as i64
    }
}

pub(crate) struct FilesState {
    /// The file open in the pane, per thread.
    pub open: HashMap<String, OpenFile>,
    /// Paths the runner has said are folders, so a click goes to the browser.
    known_folders: HashSet<String>,
    /// The pane's own text style, built once per appearance.
    /// Keyed by appearance and zoom, as `rows`' is.
    reading_style: Option<((bool, u32), TextViewStyle)>,
    /// How files and folders show, remembered across launches.
    pub(crate) prefs: FilePrefs,
    /// What was last looked for, which the next ⌘F starts with, as AppKit's
    /// find pasteboard has it.
    pub(crate) last_query: String,
    /// The file's own focus, so ⌘F reaches the pane with nothing in it picked.
    pub(crate) focus: Option<FocusHandle>,
    /// The window the pane draws in, for putting a file's text in its editor
    /// after a save or a check that came later.
    pub(crate) window: Option<AnyWindowHandle>,
}

impl Default for FilesState {
    fn default() -> Self {
        FilesState { open: HashMap::new(), known_folders: HashSet::new(), reading_style: None, prefs: crate::files_store::load(), last_query: String::new(), focus: None, window: None }
    }
}

impl FilesState {
    pub(crate) fn is_known_folder(&self, path: &str) -> bool {
        self.known_folders.contains(path)
    }
}

/// What's drawn for a file.
pub(crate) enum FileBody {
    /// A spinner would only flash for a normal file; a beat later there's a note.
    Loading,
    Failed { message: String },
    Markdown { doc: Entity<TextViewState>, text: Arc<str> },
    /// Code or plain text: a read-only editor, coloured for code. The editor
    /// is made at the next frame, as it needs a window.
    Text { editor: Option<Entity<EditorState>>, text: Arc<str>, language: Option<&'static str> },
    Picture { image: Arc<Image>, size: Option<(u32, u32)> },
    /// Something only its own app can show.
    Opaque { note: String },
    /// A video or a PDF on this Mac, in the system's own view over the pane
    /// (media_view.rs). Nothing's read for it here: the view reads the real
    /// file the open gate found, as it's made.
    Media(crate::media_view::Kind),
}

/// Which of the system's views a file in the pane shows in, by its name: a
/// video or a PDF, on a Mac. None elsewhere, where they open in their app.
pub(crate) fn media_kind(path: &str) -> Option<crate::media_view::Kind> {
    use crate::media_view::Kind;
    use threadbear_core::open_policy::Media;
    if !crate::media_view::SHOWN_HERE {
        return None;
    }
    threadbear_core::open_policy::media_kind(path).map(|m| match m {
        Media::Video => Kind::PaneVideo,
        Media::Pdf => Kind::PaneDocument,
    })
}

/// The pane's view for this thread's file, opened this many times: opened
/// again from a row, it plays again from the start, as the Swift pane's does.
pub(crate) fn media_key(thread_id: &str, path: &str, opened: u64) -> String {
    format!("pane {thread_id} {opened} {path}")
}

/// Whether the pane is showing a video or a PDF in the system's view, which
/// has to be placed every frame (shell.rs draws the pane uncached then).
pub(crate) fn shows_media(side: &SidePane, thread_id: &str) -> bool {
    let Some(path) = side.previews.get(thread_id).and_then(|p| p.file_path()) else { return false };
    side.files.open.get(thread_id).is_some_and(|f| f.path == path && matches!(f.body, FileBody::Media(_)))
}

/// An image's format from its name, for the kit's decoder.
pub(crate) fn image_format(path: &str) -> Option<ImageFormat> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "bmp" => ImageFormat::Bmp,
        "tif" | "tiff" => ImageFormat::Tiff,
        "svg" => ImageFormat::Svg,
        _ => return None,
    })
}

/// `%20` and the like, in a file link.
pub(crate) fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 {
            if let Some(v) = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn home_dir() -> String {
    // Windows sets USERPROFILE; HOME only exists there under Git Bash.
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default()
}

/// A web page in the browser (the panel page is later work). Only http and
/// https: anything else is refused by the open gate, said so.
pub(crate) fn open_web(url: &str, cx: &mut App) {
    crate::open_gate::open_web(url, cx);
}

/// A file on this runner in its own app, through the open gate: a document
/// opens, a program asks first (OpenGate's rule).
pub(crate) fn open_file_in_app(path: &str, origin: crate::open_gate::Origin, cx: &mut App) {
    crate::open_gate::open_file(path, origin, cx);
}

pub(crate) fn reveal(path: &str, origin: crate::open_gate::Origin, cx: &mut App) {
    crate::open_gate::reveal(path, origin, cx);
}

/// A quiet line in the middle of the pane (`PaneNote`).
pub(crate) fn pane_note(text: &str, p: Palette) -> impl IntoElement {
    v_flex().size_full().items_center().justify_center().p(px(24.)).child(div().text_center().text_size(px(style::size::LABEL)).text_color(p.secondary).child(text.to_owned()))
}

/// What the pane says for a file the runner couldn't read: one that's gone
/// (renamed away, deleted, perhaps while it showed) as the Swift app says it.
fn failed_words(message: &str) -> String {
    if message.starts_with("No such file") {
        threadbear_core::text_file::words::GONE.to_owned()
    } else {
        format!("Couldn't read it: {message}")
    }
}

/// An ask's words without their markdown (`plainAction`).
pub(crate) fn plain_markdown(text: &str) -> String {
    threadbear_core::sidebar::preview_plain(text)
}

/// The label of the bar's Preview/Markdown menu (`MarkdownViewMenu`): its
/// pick and a small chevron, grey until the pointer's on it, then a rounded
/// fill behind.
#[derive(IntoElement)]
pub(crate) struct ModeLabel {
    inner: Stateful<Div>,
}

impl ModeLabel {
    fn new(id: &'static str, title: &str, p: Palette) -> Self {
        let (hover, fg) = (p.hover, p.fg);
        let inner = div()
            .id(id)
            .flex_none()
            .h(px(26.))
            .px(px(7.))
            .flex()
            .items_center()
            .gap(px(3.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(px(style::size::SMALL))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.secondary)
            .hover(move |s| s.bg(hover).text_color(fg))
            .child(title.to_owned())
            .child(icons::symbol("chevron.down").with_size(px(style::size::SMALL - 2.)));
        ModeLabel { inner }
    }
}

impl RenderOnce for ModeLabel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.inner
    }
}

impl Styled for ModeLabel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl InteractiveElement for ModeLabel {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl Selectable for ModeLabel {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl DropdownMenu for ModeLabel {}

impl SidePane {
    /// Reads the file the pane's preview names, if it's one that shows here.
    pub(crate) fn begin_preview(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id).cloned() else { return };
        match &preview.target {
            Target::File(path) => match core::viewer_for_path(path) {
                // A page in the system browser for now, and the preview
                // closes. Another Mac's file has no copy here to open.
                Viewer::Web if thread.is_remote() => {
                    let note = "Web pages from another Mac can't be opened from here yet.".to_owned();
                    self.files.open.insert(thread.id.clone(), OpenFile::new(path.clone(), preview.opened, FileBody::Opaque { note }));
                    cx.notify();
                }
                // The real file, after its symlinks: a page named .html that's
                // really something else asks first, as any file does.
                Viewer::Web => {
                    open_file_in_app(path, crate::open_gate::Origin::ThisMac, cx);
                    self.previews.remove(&thread.id);
                    cx.notify();
                }
                _ if media_kind(path).is_some() => self.show_media_file(thread, path, preview.opened, cx),
                _ => self.load_file(thread, path.clone(), preview.opened, cx),
            },
            Target::Page(url) => {
                open_web(url, cx);
                self.previews.remove(&thread.id);
                cx.notify();
            }
            Target::Folder(_) => self.load_folder_for_preview(thread, cx),
        }
    }

    /// A video or a PDF: in the system's view over the pane, made as it's
    /// drawn, if the open gate's rules let it (this Mac's own, a real one);
    /// else why not, in its place.
    fn show_media_file(&mut self, thread: &ThreadSummary, path: &str, opened: u64, cx: &mut Context<Self>) {
        if self.files.open.get(&thread.id).is_some_and(|f| f.path == path && f.opened == opened) {
            return;
        }
        let Some(kind) = media_kind(path) else { return };
        let origin = if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
        let body = match threadbear_core::open_policy::decide_media(path, origin) {
            threadbear_core::open_policy::MediaVerdict::Show(_) => FileBody::Media(kind),
            threadbear_core::open_policy::MediaVerdict::Refuse(note) => FileBody::Opaque { note },
        };
        self.files.open.insert(thread.id.clone(), OpenFile::new(path.to_owned(), opened, body));
        cx.notify();
    }

    fn load_file(&mut self, thread: &ThreadSummary, path: String, opened: u64, cx: &mut Context<Self>) {
        // The same open again (a repaint, a re-select) reads nothing new;
        // opening it again from a row does (`opened` moved).
        if self.files.open.get(&thread.id).is_some_and(|f| f.path == path && f.opened == opened) {
            return;
        }
        self.read_file(thread, path, opened, cx);
    }

    /// The file showing, read again where it is: its folder heard it change,
    /// or its own text was put away (Done).
    pub(crate) fn reread_file(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread(cx).filter(|t| t.id == thread_id) else { return };
        let Some((path, opened)) = self.files.open.get(thread_id).map(|f| (f.path.clone(), f.opened)) else { return };
        if self.previews.get(thread_id).and_then(|p| p.file_path()) != Some(path.as_str()) {
            return;
        }
        self.read_file(&thread, path, opened, cx);
    }

    /// Asks the runner for the file's bytes.
    fn read_file(&mut self, thread: &ThreadSummary, path: String, opened: u64, cx: &mut Context<Self>) {
        let keep_previous = self.files.open.get(&thread.id).is_some_and(|f| f.path == path);
        let here = !thread.is_remote();
        if !keep_previous {
            self.files.open.insert(thread.id.clone(), OpenFile::new(path.clone(), opened, FileBody::Loading));
            cx.notify();
        }
        let client = self.client.clone();
        let thread_id = thread.id.clone();
        let mut params = json!({ "path": path, "maxBytes": MAX_BYTES });
        if let Some(runner) = &thread.runner_id {
            params["runnerId"] = json!(runner);
        }
        cx.spawn(async move |this, cx| {
            let res = call(client, "files.read", params).await;
            this.update(cx, |this, cx| {
                // A newer file opened while this one was on its way.
                let current = this.previews.get(&thread_id).and_then(|p| p.file_path().map(str::to_owned));
                if current.as_deref() != Some(path.as_str()) {
                    return;
                }
                match res {
                    Ok(v) => match FileRead::decode(&v) {
                        Some(read) => this.show_read(&thread_id, &path, opened, read, here, cx),
                        None => this.fail_file(&thread_id, &path, opened, "Couldn't read that answer from the runner.".into(), cx),
                    },
                    Err(e) if e.message.starts_with("Not a file") => {
                        // A folder: into the browser, as a click on one is.
                        this.files.known_folders.insert(path.clone());
                        this.files.open.remove(&thread_id);
                        if let Some(p) = this.previews.get_mut(&thread_id) {
                            p.target = Target::Folder(path.clone());
                        }
                        if let Some(thread) = this.thread(cx) {
                            this.begin_preview(&thread, cx);
                        }
                        cx.notify();
                    }
                    Err(e) => this.fail_file(&thread_id, &path, opened, e.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn fail_file(&mut self, thread_id: &str, path: &str, opened: u64, message: String, cx: &mut Context<Self>) {
        self.files.open.insert(thread_id.to_owned(), OpenFile::new(path.to_owned(), opened, FileBody::Failed { message }));
        cx.notify();
    }

    fn show_read(&mut self, thread_id: &str, path: &str, opened: u64, read: FileRead, here: bool, cx: &mut Context<Self>) {
        let viewer = core::viewer_for_path(path);
        let is_picture = viewer == Viewer::Picture && (core::looks_like_picture(path) || image_format(path).is_some());
        let body = if is_picture {
            match image_format(path) {
                Some(format) => {
                    FileBody::Picture { size: picture_size(&read.data), image: Arc::new(Image::from_bytes(format, read.data)) }
                }
                None => FileBody::Opaque { note: "This kind of picture opens in its own app.".into() },
            }
        } else {
            let text_ok = viewer == Viewer::Text || core::looks_like_text(&read.data);
            let opaque = viewer != Viewer::Text && !core::looks_like_text(&read.data);
            if opaque || !text_ok {
                FileBody::Opaque { note: "This file opens in its own app.".into() }
            } else {
                match String::from_utf8(read.data) {
                    Err(_) => FileBody::Opaque { note: "This file isn't text this app can read.".into() },
                    Ok(text) => self.text_body(path, text, cx),
                }
            }
        };
        // A file read again (opened again from a row, or changed on disk) keeps
        // its find bar, its own text being worked on (checked against the
        // file, as `check` does), the page where it was, and the watch on it.
        let previous = self.files.open.remove(thread_id).filter(|f| f.path == path);
        let body = match (body, previous.as_ref().map(|p| &p.body)) {
            // The same words: nothing to draw again, and nothing moves.
            (FileBody::Markdown { text, .. }, Some(FileBody::Markdown { doc, text: was })) if text == *was => FileBody::Markdown { doc: doc.clone(), text },
            (FileBody::Text { text, language, .. }, Some(FileBody::Text { editor, text: was, .. })) if text == *was => FileBody::Text { editor: editor.clone(), text, language },
            // New words on the same page: read again where it was.
            (FileBody::Markdown { text, doc: fresh }, Some(FileBody::Markdown { doc, .. })) => {
                drop(fresh);
                let source = text.to_string();
                doc.update(cx, |s, cx| s.set_text(&source, cx));
                FileBody::Markdown { doc: doc.clone(), text }
            }
            (body, _) => body,
        };
        let page_watch = match &body {
            // The find bar searches what the page draws, which lands after
            // its parse: it searches again when the page changes.
            FileBody::Markdown { doc, .. } => {
                let id = thread_id.to_owned();
                Some(cx.observe(doc, move |this, _, cx| this.page_changed(&id, cx)))
            }
            _ => None,
        };
        let text = matches!(body, FileBody::Markdown { .. } | FileBody::Text { .. });
        let mut open = OpenFile::new(path.to_owned(), opened, body);
        let mut had_edit = false;
        // Its own text stays only while the preview's still working on it
        // (opened again from a row, it reads, as the Swift app's does).
        let editing = self.previews.get(thread_id).is_some_and(|p| p.editing);
        if let Some(previous) = previous {
            open.find = previous.find;
            open.page_scroll = previous.page_scroll;
            if editing {
                had_edit = previous.edit.is_some();
                open.edit = previous.edit;
            }
            open._file_watch = previous._file_watch;
        }
        // Text on this machine is watched, so an agent's rewrite shows up by
        // itself; another Mac's is that Mac's to say.
        if here && text && open._file_watch.is_none() {
            open._file_watch = self.watch_file(thread_id, path, cx);
        }
        open.truncated = read.truncated;
        open.size = read.size;
        open._page_watch = page_watch;
        self.files.open.insert(thread_id.to_owned(), open);
        if had_edit {
            self.check_edit(thread_id, cx);
        }
        // The new text under an open find bar.
        if self.files.open.get(thread_id).is_some_and(|o| o.find.is_some()) {
            self.find_changed(thread_id, cx);
        }
        cx.notify();
    }

    /// A watch on the file's folder that reads the file again when it
    /// changes, a moment after the burst settles. Its own text being worked
    /// on has a watch of its own (file_edit.rs).
    fn watch_file(&mut self, thread_id: &str, path: &str, cx: &mut Context<Self>) -> Option<FolderWatch> {
        let file = std::path::Path::new(path).file_name().map(|n| n.to_owned());
        let folder = std::path::Path::new(path).parent()?.to_string_lossy().into_owned();
        let id = thread_id.to_owned();
        let mut watch = FolderWatch::new(cx, move |this: &mut SidePane, _, paths, cx| {
            if !paths.iter().any(|p| p.file_name() == file.as_deref()) {
                return;
            }
            let Some(open) = this.files.open.get_mut(&id).filter(|o| o.edit.is_none()) else { return };
            // One read after the burst: a newer change puts the read off.
            let later = id.clone();
            open._reread = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(threadbear_core::text_file::CHECK_DELAY_MS)).await;
                this.update(cx, |this, cx| this.reread_file(&later, cx)).ok();
            }));
        })?;
        watch.set(&HashSet::from([folder]));
        Some(watch)
    }

    /// Markdown drawn, code coloured, text plain, all of it selectable.
    fn text_body(&mut self, path: &str, text: String, cx: &mut Context<Self>) -> FileBody {
        let text: Arc<str> = text.into();
        if core::is_markdown(path) {
            let source = text.to_string();
            let doc = cx.new(|cx| TextViewState::markdown(&source, cx));
            return FileBody::Markdown { doc, text };
        }
        let language = core::code_language(path).filter(|_| text.len() < HIGHLIGHT_LIMIT);
        // The editor is made at the next frame, with a window (render_file).
        FileBody::Text { editor: None, text, language }
    }

    /// A previewed Markdown file, its links going through the open gate:
    /// a file it links to (from the file's own folder) previews here too,
    /// read through the runner, so a repo's page can't hand the system one
    /// of its programs; a web page opens; anything else asks.
    fn preview_text(&self, doc: &Entity<TextViewState>, thread: &ThreadSummary, path: &str, cx: &mut Context<Self>) -> gpui_kit::base::TextView {
        use threadbear_core::open_policy::{self as policy, LinkKind};
        let origin = if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
        let folder = path.rsplit_once(['/', '\\']).map_or(String::new(), |(dir, _)| dir.to_owned());
        let (side, thread) = (cx.entity().downgrade(), thread.clone());
        let base = folder.clone();
        crate::open_gate::text_view(doc, origin, &folder).on_link_click(move |url: &SharedString, _: &ClickEvent, _: &mut Window, cx: &mut App| match policy::link_kind(url) {
            LinkKind::File(target) => {
                if let Some(target) = policy::absolute(&target, &base, &home_dir()) {
                    let _ = side.update(cx, |side, cx| side.preview_path(&target, None, &thread, cx));
                }
            }
            _ => crate::open_gate::open_link(url, origin, &base, cx),
        })
    }

    /// Whether the bar offers the file's own text (the Markdown menu): a
    /// whole text file on this runner's machine. Another Mac's would only be
    /// a copy here (`PreviewButtons`, `preview.away == nil`).
    fn offers_text(&self, preview: &PanePreview, thread: &ThreadSummary) -> bool {
        let Some(path) = preview.file_path() else { return false };
        !thread.is_remote() && self.files.open.get(&thread.id).is_some_and(|f| f.path == path && f.is_whole_text())
    }

    /// The bar's buttons over a preview: the Markdown menu, Reveal, Open
    /// (`PreviewButtons`).
    pub(crate) fn preview_buttons(&mut self, preview: &PanePreview, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = h_flex().flex_none().gap(px(0.));
        if let Target::Folder(path) = &preview.target {
            row = row.child(self.folder_mode_button(p, cx));
            // Another Mac's folder has no copy here to hand over.
            if !thread.is_remote() {
                let open_path = path.clone();
                row = row.child(self.header_button("hl-open", "arrow.up.right", "Open in Finder".to_owned(), p, cx, move |this, cx| this.open_externally(&open_path.clone(), cx)));
            }
        }
        // A file here that's whole text: the Markdown menu for Markdown, else
        // the pencil, and Done while it's being worked on. Another Mac's file
        // isn't edited here, as only the copy would change.
        if let Some(path) = preview.file_path().filter(|_| self.offers_text(preview, thread)) {
            if core::is_markdown(path) {
                row = row.child(self.markdown_menu(preview.editing, p, cx));
            } else {
                let (editing, id) = (preview.editing, thread.id.clone());
                let (symbol, tip) = if editing { ("checkmark", words::DONE_TIP) } else { ("pencil", words::EDIT_TIP) };
                row = row.child(self.header_button("hl-edit", symbol, tip.to_owned(), p, cx, move |this, cx| this.request_editing(&id, !editing, cx)));
            }
        }
        if let Some(path) = preview.file_path().filter(|_| !thread.is_remote()).map(str::to_owned) {
            let reveal_path = path.clone();
            row = row.child(self.header_button("hl-reveal", "folder", REVEAL_TITLE.to_owned(), p, cx, move |_, cx| reveal(&reveal_path.clone(), crate::open_gate::Origin::ThisMac, cx)));
            let open_path = path.clone();
            row = row.child(self.header_button("hl-open", "arrow.up.right", "Open in its app".to_owned(), p, cx, move |this, cx| this.open_externally(&open_path.clone(), cx)));
        }
        row
    }

    /// `MarkdownViewMenu`: Preview or Markdown, the pick remembered for the
    /// next Markdown file.
    fn markdown_menu(&self, editing: bool, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let showing = if editing { MarkdownView::Markdown } else { MarkdownView::Preview };
        let tip = showing.tip();
        let side = cx.entity().downgrade();
        let holder = crate::highlights::tipped(div().id("hl-markdown-view").flex_none(), tip);
        let menu = ModeLabel::new("hl-markdown-view-label", showing.title(), p).dropdown_menu(move |menu: PopupMenu, _, _| {
            let mut menu = menu;
            for view in MarkdownView::ALL {
                let side = side.clone();
                let item = PopupMenuItem::new(view.title()).checked(view == showing).icon(icons::symbol(view.symbol())).on_click(move |_, window, cx| {
                    side.update(cx, |this, cx| this.pick_markdown_view(view, window, cx)).ok();
                });
                menu = menu.item(item);
            }
            menu
        });
        holder.child(menu).into_any_element()
    }

    /// A pick from the Markdown menu: this file shows that way, and so does
    /// the next one opened (`MarkdownView.current`).
    pub(crate) fn pick_markdown_view(&mut self, view: MarkdownView, window: &mut Window, cx: &mut Context<Self>) {
        self.files.prefs.markdown_view = view;
        crate::files_store::save(&self.files.prefs);
        let Some(thread) = self.thread(cx) else { return };
        self.set_preview_editing(&thread, view == MarkdownView::Markdown, window, cx);
    }

    /// The pencil or Done, from the bar, where there's no window to hand:
    /// the change waits for the pane's next frame, which has one.
    fn request_editing(&mut self, thread_id: &str, editing: bool, cx: &mut Context<Self>) {
        let Some(handle) = self.files.window else { return };
        let (side, id) = (cx.entity().downgrade(), thread_id.to_owned());
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                side.update(cx, |this, cx| {
                    if let Some(thread) = this.thread(cx).filter(|t| t.id == id) {
                        this.set_preview_editing(&thread, editing, window, cx);
                    }
                })
            });
        });
    }

    /// The file's own text, shown to type in or put away: the pencil and Done,
    /// or the Markdown menu's pick. Done while what's typed can't be saved (a
    /// conflict waiting, a file that can't take it) stays editing, so the
    /// typing is still there for the conflict bar or another try.
    pub(crate) fn set_preview_editing(&mut self, thread: &ThreadSummary, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id) else { return };
        if preview.file_path().is_none() || preview.editing == editing {
            return;
        }
        if !editing {
            if let Some(edit) = self.files.open.get_mut(&thread.id).and_then(|o| o.edit.as_mut()) {
                let typed = edit.editor.read(cx).value().to_string();
                if !edit.session.set_editing(false, &typed) {
                    cx.notify();
                    return;
                }
            }
        }
        if let Some(preview) = self.previews.get_mut(&thread.id) {
            preview.editing = editing;
        }
        let had_find = self.files.open.get(&thread.id).is_some_and(|o| o.find.is_some());
        if !editing {
            let had_edit = self.files.open.get_mut(&thread.id).and_then(|o| o.edit.take()).is_some();
            // What was saved shows as it now reads.
            if had_edit {
                self.reread_file(&thread.id, cx);
            }
        }
        self.ensure_edit(thread, window, cx);
        // The find bar stays, searching what shows now.
        if had_find {
            self.find_changed(&thread.id, cx);
        }
        cx.notify();
    }

    /// The file's own text for a preview that's showing it, made once there's
    /// a window to make its editor with.
    fn ensure_edit(&mut self, thread: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id).cloned() else { return };
        if !preview.editing || !self.offers_text(&preview, thread) {
            return;
        }
        let Some(open) = self.files.open.get(&thread.id) else { return };
        if open.edit.is_some() {
            return;
        }
        let path = open.path.clone();
        let language = core::code_language(&path).filter(|_| open.size < HIGHLIGHT_LIMIT as i64);
        let edit = FileEdit::open(&thread.id, &path, language, window, cx);
        if let Some(open) = self.files.open.get_mut(&thread.id) {
            open.edit = Some(edit);
        }
    }

    pub(crate) fn render_preview(&mut self, thread: &ThreadSummary, preview: &PanePreview, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        match &preview.target {
            Target::File(path) => self.render_file(thread, path, preview.editing, window, cx),
            Target::Page(url) => pane_note(&format!("{url} opened in your browser."), p).into_any_element(),
            Target::Folder(path) => self.render_folder(thread, path, preview, window, cx),
        }
    }

    fn render_file(&mut self, thread: &ThreadSummary, path: &str, editing: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // A file's text that's waiting for its editor gets one now.
        if let Some(open) = self.files.open.get_mut(&thread.id).filter(|f| f.path == path) {
            if let FileBody::Text { editor, text, language } = &mut open.body {
                if editor.is_none() {
                    let (value, language) = (text.to_string(), *language);
                    let made = cx.new(|cx| {
                        // Reading, as the Swift app's text view is until its
                        // pencil: no line numbers, folds or guides, and ⌘F
                        // left to the pane's own find bar.
                        let state = EditorState::new(window, cx).soft_wrap(true).line_number(false).folding(false).indent_guides(false).searchable(false);
                        match language {
                            Some(language) => state.language(language),
                            None => state,
                        }
                    });
                    made.update(cx, |state, cx| state.set_value(value, window, cx));
                    *editor = Some(made);
                }
            }
        }
        if editing {
            self.ensure_edit(thread, window, cx);
        }
        self.files.window = Some(window.window_handle());
        let focus = self.files.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let Some(open) = self.files.open.get(&thread.id).filter(|f| f.path == path) else {
            return div().size_full().into_any_element();
        };
        let thread_id = thread.id.clone();
        if let Some(edit) = open.edit.as_ref().filter(|_| editing) {
            let body = self.render_edit(&thread_id, edit, cx);
            return self.file_keys(div().id("hl-file").size_full().track_focus(&focus).child(body), &thread_id, cx).into_any_element();
        }
        let cut = open.truncated.then(|| div().flex_none().w_full().py(px(6.)).text_center().text_size(px(style::size::SMALL)).text_color(p.secondary).child("Only the start of this file came from the runner."));
        let body: AnyElement = match &open.body {
            // A spinner so a click shows something at once; the header already
            // says which file it's getting.
            FileBody::Loading => div().size_full().flex().items_center().justify_center().child(Spinner::new().with_size(px(14.)).color(p.secondary)).into_any_element(),
            FileBody::Failed { message } => pane_note(&failed_words(message), p).into_any_element(),
            FileBody::Opaque { note } => pane_note(note, p).into_any_element(),
            FileBody::Media(kind) => {
                let kind = *kind;
                let origin = if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
                let key = media_key(&thread.id, path, open.opened);
                let path = path.to_owned();
                // The view's slot is the whole pane; a film narrower than it
                // sits in the middle with the pane's colour round it, as the
                // Swift pane's player does.
                match crate::media_view::slot(&key, window, |opening| crate::open_gate::show_media(&path, origin, kind, opening)) {
                    // A PDF still being read off the main thread has the
                    // pane's spinner where it will be, until it's in.
                    Ok(view) if crate::media_view::state_of(&key).is_some_and(|s| s.loading) => div()
                        .id("hl-media")
                        .size_full()
                        .relative()
                        .child(view)
                        .child(div().absolute().inset_0().flex().items_center().justify_center().child(Spinner::new().with_size(px(14.)).color(p.secondary)))
                        .into_any_element(),
                    Ok(view) => div().id("hl-media").size_full().child(view).into_any_element(),
                    Err(why) => pane_note(&why, p).into_any_element(),
                }
            }
            FileBody::Picture { image, .. } => {
                let source = ImageSource::Image(image.clone());
                let (path2, entity) = (path.to_owned(), cx.entity().downgrade());
                // Dragged, it's the file itself, from this Mac's own threads
                // only (file_drag.rs, `FilePreviewView`'s drag).
                let drag = Some(path.to_owned()).filter(|_| !thread.is_remote());
                crate::file_drag::FileDragExt::file_drag(div().id("hl-picture"), drag)
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(0.))
                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                        // Copy Image, the picture's own right-click (`FilePreviewView.menu`).
                        let position = event.position;
                        let path3 = path2.clone();
                        entity
                            .update(cx, |this, cx| this.open_picture_menu(path3, position, window, cx))
                            .ok();
                    })
                    .child(img(source).size_full().object_fit(ObjectFit::Contain))
                    // Found by scripts, for a drag from it.
                    .test_support()
                    .into_any_element()
            }
            FileBody::Markdown { doc, .. } => {
                let key = (cx.theme().is_dark(), crate::style::zoom_percent());
                let style = match &self.files.reading_style {
                    Some((was, style)) if *was == key => style.clone(),
                    _ => {
                        let built = crate::rows::reading_style(cx);
                        self.files.reading_style = Some((key, built.clone()));
                        built
                    }
                };
                let scroll = open.page_scroll.clone();
                let reveal = scroll.clone();
                // A page, with margins and lines no longer than the Swift
                // app's pageWidth however wide the panel is.
                div()
                    .id("hl-markdown")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .max_w(px(720.))
                            .mx_auto()
                            .px(px(24.))
                            .py(px(18.))
                            .text_size(px(style::size::BODY))
                            .line_height(px(style::size::BODY_LINE))
                            .child(self.preview_text(doc, thread, path, cx).selectable(true).style(style).on_reveal(move |line, _, _| {
                                // The find bar's match, scrolled into the pane.
                                let viewport = reveal.bounds();
                                let mut offset = reveal.offset();
                                if line.bottom() > viewport.bottom() {
                                    offset.y -= line.bottom() - viewport.bottom();
                                } else if line.top() < viewport.top() {
                                    offset.y += viewport.top() - line.top();
                                }
                                reveal.set_offset(offset);
                            })),
                    )
                    .into_any_element()
            }
            FileBody::Text { editor: Some(editor), .. } => div()
                .size_full()
                .child(
                    Editor::new(editor)
                        .readonly(true)
                        .h_full()
                        .appearance(false)
                        .bordered(false)
                        .text_size(px(style::size::DETAIL))
                        // AppKit's line for the 12 point monospaced font, not the kit's 18.
                        .line_height(px(CODE_LINE))
                        .font_family(cx.theme().mono_font_family.clone()),
                )
                .into_any_element(),
            FileBody::Text { .. } => div().size_full().into_any_element(),
        };
        let find = self.render_find_bar(&thread_id, cx);
        let column = v_flex().size_full().when_some(cut, |s, c| s.child(c)).children(find).child(div().flex_1().min_h_0().child(body));
        self.file_keys(div().id("hl-file").size_full().track_focus(&focus).child(column), &thread_id, cx).into_any_element()
    }

    /// The keys a file in the pane takes: ⌘F, ⌘G and ⇧⌘G for the find bar.
    /// The keys a file in the pane takes: ⌘F, ⌘G and ⇧⌘G for the find bar,
    /// ⌘S to save what's typed now rather than after the pause.
    fn file_keys(&self, el: Stateful<Div>, thread_id: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let (find, next, previous, save) = (thread_id.to_owned(), thread_id.to_owned(), thread_id.to_owned(), thread_id.to_owned());
        el.key_context(crate::file_find::CONTEXT)
            .on_action(cx.listener(move |this, _: &gpui_kit::component::input::Search, window, cx| this.open_find(&find, window, cx)))
            .on_action(cx.listener(move |this, _: &FindNext, window, cx| this.find_again(&next, false, window, cx)))
            .on_action(cx.listener(move |this, _: &FindPrevious, window, cx| this.find_again(&previous, true, window, cx)))
            .on_action(cx.listener(move |this, _: &SaveFile, window, cx| {
                // Only while it's being worked on, as the Swift app's ⌘S.
                if this.files.open.get(&save).is_some_and(|o| o.edit.as_ref().is_some_and(|e| e.session.takes_typing())) {
                    this.save_edit(&save, Some(window), cx);
                } else {
                    cx.propagate();
                }
            }))
    }

    /// `Copy Image`: the file's own bytes, not a path, so it pastes as the picture.
    fn open_picture_menu(&mut self, _path: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let items = vec![crate::highlights::MenuItem::picture_copy()];
        self.open_menu(items, position, window, cx);
    }

    /// Puts the picture on the clipboard as the file's own bytes (a PNG stays a
    /// PNG), which every app that takes a pasted picture reads (`copyImage`).
    pub(crate) fn copy_open_picture(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let thread = self.thread(cx)?;
        let open = self.files.open.get(&thread.id)?;
        let FileBody::Picture { image, .. } = &open.body else { return None };
        let format = match image.format() {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Webp => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Svg => "svg",
            _ => "image",
        };
        cx.write_to_clipboard(ClipboardItem::new_image(image));
        Some(format.to_owned())
    }

    /// `dump`'s preview line (`Panels.preview`).
    #[cfg(feature = "automation")]
    pub(crate) fn preview_line(&self, preview: &PanePreview) -> String {
        match &preview.target {
            Target::File(path) => format!("file {path}{}", if preview.editing { " (editing)" } else { "" }),
            Target::Folder(path) => format!("folder {path}"),
            Target::Page(url) => format!("page {url}"),
        }
    }

    #[cfg(feature = "automation")]
    pub(crate) fn fill_preview_dump(&self, pane: &mut serde_json::Map<String, Value>, preview: &PanePreview, cx: &App) {
        match &preview.target {
            Target::Page(url) => {
                pane.insert("previewKind".into(), json!("page"));
                pane.insert("previewText".into(), json!(url));
            }
            Target::Folder(path) => {
                pane.insert("previewKind".into(), json!("folder"));
                pane.insert("previewText".into(), json!(path));
            }
            Target::File(path) => {
                let Some(thread) = self.thread(cx) else { return };
                let Some(open) = self.files.open.get(&thread.id).filter(|f| &f.path == path) else { return };
                if let Some(find) = self.find_line(&thread.id, cx) {
                    pane.insert("previewFind".into(), json!(find));
                }
                // What the bar offers: the Markdown menu, or the pencil and
                // Done (PreviewButtons).
                if self.offers_text(preview, &thread) {
                    let mode = match (core::is_markdown(path), preview.editing) {
                        (true, true) => "markdown",
                        (true, false) => "preview",
                        (false, true) => "editing",
                        (false, false) => "reading",
                    };
                    pane.insert("previewMode".into(), json!(mode));
                }
                if let Some(edit) = open.edit.as_ref().filter(|_| preview.editing) {
                    pane.insert("previewKind".into(), json!(if core::is_markdown(path) || core::code_language(path).is_none() { "text" } else { "code" }));
                    pane.insert("previewText".into(), json!(edit.editor.read(cx).value().chars().take(4000).collect::<String>()));
                    let note = if edit.session.conflict.is_some() { Some(threadbear_core::text_file::words::CONFLICT.to_owned()) } else { edit.session.note.clone() };
                    if let Some(note) = note {
                        pane.insert("previewNote".into(), json!(note));
                    }
                    return;
                }
                match &open.body {
                    FileBody::Loading => {}
                    FileBody::Failed { message } => {
                        pane.insert("previewKind".into(), json!("note"));
                        pane.insert("previewText".into(), json!(failed_words(message)));
                    }
                    FileBody::Opaque { note } => {
                        pane.insert("previewKind".into(), json!("note"));
                        pane.insert("previewText".into(), json!(note));
                    }
                    // As the Swift pane says it (FilePreviewView.debugShows):
                    // "video" with the film's size once its track has said;
                    // a PDF, its pages.
                    FileBody::Media(kind) => {
                        let key = media_key(&thread.id, path, open.opened);
                        if let Some(why) = crate::media_view::failure_of(&key) {
                            pane.insert("previewKind".into(), json!("note"));
                            pane.insert("previewText".into(), json!(why));
                        } else if *kind == crate::media_view::Kind::PaneDocument {
                            pane.insert("previewKind".into(), json!("pdf"));
                            if let Some(state) = crate::media_view::state_of(&key).filter(|s| !s.loading) {
                                pane.insert("previewText".into(), json!(format!("{} page{}", state.pages, if state.pages == 1 { "" } else { "s" })));
                            }
                        } else {
                            pane.insert("previewKind".into(), json!("video"));
                            if let Some(state) = crate::media_view::state_of(&key) {
                                if state.width > 0. && state.height > 0. {
                                    pane.insert("previewText".into(), json!(format!("{}x{}", state.width.round(), state.height.round())));
                                }
                                pane.insert("previewVideo".into(), json!({ "playing": state.playing, "muted": state.muted, "controls": state.controls, "looping": state.looping }));
                            }
                        }
                    }
                    FileBody::Picture { size, .. } => {
                        pane.insert("previewKind".into(), json!("picture"));
                        pane.insert("previewText".into(), json!(size.map(|(w, h)| format!("{w}x{h}"))));
                    }
                    FileBody::Markdown { text, .. } => {
                        pane.insert("previewKind".into(), json!("markdown"));
                        pane.insert("previewText".into(), json!(text.chars().take(4000).collect::<String>()));
                    }
                    FileBody::Text { text, language, .. } => {
                        pane.insert("previewKind".into(), json!(if language.is_some() { "code" } else { "text" }));
                        pane.insert("previewText".into(), json!(text.chars().take(4000).collect::<String>()));
                    }
                }
            }
        }
    }
}

/// The `preview` automation command (`Automation.swift`). Over a text file,
/// "find" is ⌘F with those words typed in the find bar, "findNext" and
/// "findPrevious" are ⌘G and ⇧⌘G, "findDone" the bar's Done, each saying what
/// the bar has found; "markdownView" is a pick from a Markdown file's menu;
/// "edit" true or false is the pencil or Done, "insert" types text at the end
/// of the file being worked on, "save" is ⌘S, and "resolve" ("file" or
/// "mine") is the conflict bar's button, each saying how the editor and the
/// file end.
#[cfg(feature = "automation")]
pub(crate) fn preview_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    if cmd["play"].as_bool() == Some(true) {
        // The film's play button, for a picture of it playing.
        let pane = side.read(cx);
        let key = pane.files.open.get(&thread.id).filter(|o| matches!(o.body, FileBody::Media(crate::media_view::Kind::PaneVideo))).map(|o| media_key(&thread.id, &o.path, o.opened));
        return if key.is_some_and(|k| crate::media_view::play(&k)) { Ok(json!("ok playing")) } else { Err("no video is showing".into()) };
    }
    if cmd["clickLink"].as_bool() == Some(true) {
        // A click on the PDF's first link, through the window as a person's
        // comes, so where it goes is PDFKit's handling and the gate's.
        let pane = side.read(cx);
        let key = pane.files.open.get(&thread.id).filter(|o| matches!(o.body, FileBody::Media(crate::media_view::Kind::PaneDocument))).map(|o| media_key(&thread.id, &o.path, o.opened));
        return if key.is_some_and(|k| crate::media_view::click_link(&k)) { Ok(json!("ok clicked")) } else { Err("no PDF with a link is showing".into()) };
    }
    if cmd["copyImage"].as_bool() == Some(true) {
        let format = side.update(cx, |pane, cx| pane.copy_open_picture(cx)).ok_or("no picture is showing")?;
        return Ok(json!(format!("ok copied {format}")));
    }
    let id = thread.id.clone();
    // How the editor and the file on disk end, as the Swift app answers.
    let ends = |side: &Entity<SidePane>, cx: &App| -> String {
        let pane = side.read(cx);
        let Some(open) = pane.files.open.get(&id) else { return "no file".to_owned() };
        let editor = open.edit.as_ref().map(|e| e.editor.read(cx).value().to_string()).unwrap_or_default();
        let file = std::fs::read_to_string(&open.path).unwrap_or_default();
        let tail = |s: &str| s.chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<String>();
        format!("editor ends {:?}, file ends {:?}", tail(&editor), tail(&file))
    };
    let editing = |side: &Entity<SidePane>, cx: &App| side.read(cx).files.open.get(&id).is_some_and(|o| o.edit.is_some()) && side.read(cx).previews.get(&id).is_some_and(|p| p.editing);
    if cmd["save"].as_bool() == Some(true) {
        if !editing(side, cx) {
            return Err("nothing is being edited".into());
        }
        let saved = side.update(cx, |pane, cx| pane.save_edit(&id, Some(window), cx));
        let conflict = if saved == threadbear_core::text_file::Saved::Kept { "changed on disk, so not saved; " } else { "" };
        return Ok(json!(format!("ok {conflict}{}", ends(side, cx))));
    }
    if let Some(text) = cmd["insert"].as_str() {
        if !editing(side, cx) {
            return Err("nothing is being edited".into());
        }
        let editor = side.read(cx).files.open.get(&id).and_then(|o| o.edit.as_ref()).map(|e| e.editor.clone()).ok_or("nothing is being edited")?;
        // As keys would: at the end, then the save a moment later.
        editor.update(cx, |s, cx| {
            let value = s.value().to_string();
            let line = value.matches('\n').count() as u32;
            let column = value.rsplit('\n').next().map_or(0, |l| l.chars().count()) as u32;
            s.set_cursor_position(gpui_kit::component::input::Position::new(line, column), window, cx);
            s.insert(text.to_owned(), window, cx);
        });
        side.update(cx, |pane, cx| pane.edit_typed(&id, cx));
        return Ok(json!(format!("ok {}", ends(side, cx))));
    }
    if let Some(which) = cmd["resolve"].as_str() {
        let waiting = side.read(cx).files.open.get(&id).and_then(|o| o.edit.as_ref()).is_some_and(|e| e.session.conflict.is_some());
        if !waiting {
            return Err("no conflict to resolve".into());
        }
        side.update(cx, |pane, cx| if which == "file" { pane.take_files_side(&id, window, cx) } else { pane.keep_mine(&id, cx) });
        return Ok(json!(format!("ok {}", ends(side, cx))));
    }
    if let Some(on) = cmd["edit"].as_bool() {
        if side.read(cx).previews.get(&id).and_then(|p| p.file_path()).is_none() {
            return Err("no file is open in the pane".into());
        }
        side.update(cx, |pane, cx| pane.set_preview_editing(thread, on, window, cx));
        let shown = side.read(cx).previews.get(&id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
        return Ok(json!(format!("ok {shown}")));
    }
    if ["find", "findNext", "findPrevious", "findDone"].iter().any(|k| !cmd[*k].is_null()) {
        let id = thread.id.clone();
        let showing = side.read(cx).files.open.get(&id).is_some_and(|o| matches!(o.body, FileBody::Markdown { .. } | FileBody::Text { .. }) || o.edit.is_some());
        if !showing {
            return Err("no text file is showing".into());
        }
        side.update(cx, |pane, cx| {
            if let Some(query) = cmd["find"].as_str() {
                pane.find_typed(&id, query, window, cx);
            }
            if cmd["findNext"].as_bool() == Some(true) {
                pane.find_again(&id, false, window, cx);
            }
            if cmd["findPrevious"].as_bool() == Some(true) {
                pane.find_again(&id, true, window, cx);
            }
            if cmd["findDone"].as_bool() == Some(true) {
                pane.close_find(&id, window, cx);
            }
        });
        let line = side.read(cx).find_line(&id, cx).unwrap_or_else(|| "no find bar".to_owned());
        return Ok(json!(format!("ok {line}")));
    }
    if let Some(name) = cmd["markdownView"].as_str() {
        let view = MarkdownView::from_name(name).ok_or("markdownView is preview or markdown")?;
        let is_markdown = side.read(cx).previews.get(&thread.id).and_then(|p| p.file_path().map(core::is_markdown)).unwrap_or(false);
        if !is_markdown {
            return Err("no Markdown file is open in the pane".into());
        }
        side.update(cx, |pane, cx| pane.pick_markdown_view(view, window, cx));
    } else if cmd["close"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.close_preview(&thread.id, cx));
    } else if cmd["back"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.preview_back(&thread.id, cx));
    } else if let Some(path) = cmd["path"].as_str() {
        side.update(cx, |pane, cx| pane.preview_path(path, None, thread, cx));
    } else if let Some(url) = cmd["url"].as_str() {
        side.update(cx, |pane, cx| {
            let section = pane.section_for(thread);
            pane.show_preview(PanePreview::new(Target::Page(url.to_owned()), core::text::address(url), section), &thread.id, cx);
        });
    } else {
        return Err("preview needs path, url or close".into());
    }
    let shown = side.read(cx).previews.get(&thread.id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
    Ok(json!(format!("ok {shown}")))
}

/// The Files panel has no preview stored: its section shows the thread's folder.
pub(crate) fn files_section_preview(thread: &ThreadSummary) -> PanePreview {
    PanePreview::new(Target::Folder(thread.cwd.clone()), "Files".into(), FILES.to_owned())
}
