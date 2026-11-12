// What a link in a reply does when it's clicked, ⌘-clicked (Control
// elsewhere), middle-clicked, right-clicked or pointed at (rich.cmdclick,
// rich.filerefs, rich.filemenu, rich.webmenu, rich.linkpeek): TextKit.swift's
// clicked(onLink:), fileLinkMenu and hoverPeek, LinkPeekCard, and AppModel's
// openTranscriptFileLink and openTranscriptWebLink. What a click does and what
// the menus say is threadbear_core::reply_links; which file a path names is
// file_refs. Everything opens through the open gate.
//
// Which link is under the pointer: gpui-kit's text view says which link was
// clicked and nothing else, so `link_at` asks it the same way. A mouse-up of a
// button nothing in the app acts on (the side "forward" button) goes to the
// window at the pointer, and the link handler (`linked_text` in rows.rs, and
// the gate's own text views) notes the link it lands on rather than following
// it (`noted_by_probe`). It's the kit's own hit test, so it's right however
// the words wrap. Only ever done between events, and never while the left
// button's down or something's being dragged (`watch`): a GPUI click fires on
// any button's release, so a probe then would click early or end the drag. A
// right-click's menu asks with the right button down, whose release nothing
// in a reply acts on.
use crate::highlights::{PanePreview, Target};
use crate::open_gate::{self, Origin};
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use threadbear_client::config::RunnerConfig;
use threadbear_core::file_refs::{self, FileLink};
use threadbear_core::highlights as hl;
use threadbear_core::open_policy::{self as policy, LinkKind};
use threadbear_core::reducer::{Row, RowContent};
use threadbear_core::reply_links::{self as rules, ChromeProfile, FileLinkAction, LinkChoice, LinkMenuEntry, PeekPicture, WebLinkAction};

/// How long the pointer has to stop before anything asks what's under it.
const REST: Duration = Duration::from_millis(60);
/// How long it rests on a link before its card shows: longer than a chip's,
/// as links are everywhere in replies and one shouldn't pop up under every
/// pass of the pointer while Mike reads (TextKit's hoverPeek).
const PEEK_AFTER: Duration = Duration::from_millis(500);
/// The button a probe "lets go of": the side forward button. The app goes
/// forward on its press, never its release (shell.rs).
const PROBE_BUTTON: MouseButton = MouseButton::Navigate(NavigationDirection::Forward);

// ---------------------------------------------------------------- what a click does, remembered

/// The Default choices, kept in the data folder as the Swift app keeps them
/// in UserDefaults (`transcriptFileLinkClick`, `webLinkClick`), under the
/// same names.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LinkPrefs {
    file_link_click: Option<String>,
    web_link_click: Option<String>,
}

static PREFS: Mutex<Option<LinkPrefs>> = Mutex::new(None);

fn prefs_path() -> PathBuf {
    RunnerConfig::from_env().home.join("desktop-links.json")
}

fn with_prefs<R>(f: impl FnOnce(&mut LinkPrefs) -> R) -> R {
    let mut cached = PREFS.lock().unwrap_or_else(|e| e.into_inner());
    let prefs = cached.get_or_insert_with(|| std::fs::read(prefs_path()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default());
    f(prefs)
}

fn save_prefs(update: impl FnOnce(&mut LinkPrefs)) {
    let data = with_prefs(|prefs| {
        update(prefs);
        serde_json::to_vec_pretty(prefs).ok()
    });
    // Best effort: it's a preference, not data. Written whole, and only
    // this user's, as everything in the data folder is.
    if let Some(data) = data {
        let _ = threadbear_core::private_file::write(&prefs_path(), &data);
    }
}

/// What a plain click on a file link does (Default in its menu).
pub(crate) fn file_click() -> FileLinkAction {
    with_prefs(|p| p.file_link_click.as_deref().and_then(FileLinkAction::from_key)).unwrap_or_default()
}

/// What a plain click on a web link does, in a reply or Highlights' Links.
pub(crate) fn web_click() -> WebLinkAction {
    with_prefs(|p| p.web_link_click.as_deref().and_then(WebLinkAction::from_key)).unwrap_or_default()
}

pub(crate) fn set_file_click(action: FileLinkAction) {
    save_prefs(|p| p.file_link_click = Some(action.key().to_owned()));
}

pub(crate) fn set_web_click(action: WebLinkAction) {
    save_prefs(|p| p.web_link_click = Some(action.key().to_owned()));
}

// ---------------------------------------------------------------- Chrome's profiles

/// Chrome's open profiles, when Chrome is the default browser and running
/// and more than one has a window: what "Open in Chrome: <profile>" lists.
/// Read from Chrome's "Local State" each time a menu opens, as the Swift app
/// does, since Chrome rewrites it as windows open and close.
pub(crate) fn offered_profiles() -> Vec<ChromeProfile> {
    let chrome = open_gate::default_browser_app().is_some_and(|b| b.bundle_id.as_deref().is_some_and(|id| id.eq_ignore_ascii_case(rules::CHROME_BUNDLE_ID))) && open_gate::is_running(rules::CHROME_BUNDLE_ID);
    if !chrome {
        return Vec::new();
    }
    let folder = rules::chrome_data_folder(&crate::files::home_dir());
    let Ok(state) = std::fs::read_to_string(folder.join("Local State")) else { return Vec::new() };
    rules::offered_profiles(rules::chrome_profiles(&state, &folder, &|p| p.exists()), true)
}

/// The default browser's name, for "Open in Google Chrome".
pub(crate) fn browser_name() -> Option<String> {
    open_gate::default_browser_app().map(|b| b.name)
}

// ---------------------------------------------------------------- the probe

thread_local! {
    /// Set while `link_at` is asking: the link the handler landed on.
    static PROBE: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    /// Whether the left button is down, as the window's last press, release
    /// or move said (`watch`). A probe then would be taken for its release:
    /// it would click what was pressed early, or end a selection being made.
    static LEFT_HELD: Cell<bool> = const { Cell::new(false) };
}

/// Keeps `LEFT_HELD` up to date from every press, release and move in the
/// window, wherever it is. In the thread pane once; it draws nothing and
/// takes no clicks. A right press needn't count: a right-click's menu asks
/// with it held (`link_menu`), and nothing in a reply acts on its release.
pub(crate) fn watch() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|event: &MouseDownEvent, phase, _, _| {
                if phase.capture() && event.button == MouseButton::Left {
                    LEFT_HELD.with(|held| held.set(true));
                }
            });
            window.on_mouse_event(|event: &MouseUpEvent, phase, _, _| {
                if phase.capture() && event.button == MouseButton::Left {
                    LEFT_HELD.with(|held| held.set(false));
                }
            });
            // A release outside the window is still told by the next move.
            window.on_mouse_event(|event: &MouseMoveEvent, phase, _, _| {
                if phase.capture() {
                    LEFT_HELD.with(|held| held.set(event.pressed_button == Some(MouseButton::Left)));
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

/// Whether the left button is down in the window (`LEFT_HELD`): an update's
/// restart waits for the release (app_update.rs), and a draft box's words
/// step aside for a selection being made from outside them (draft_boxes.rs).
pub(crate) fn left_held() -> bool {
    LEFT_HELD.with(Cell::get)
}

/// Called first by every link handler: during a probe it notes the link and
/// says so, and the handler does nothing else.
pub(crate) fn noted_by_probe(url: &str) -> bool {
    PROBE.with(|probe| match probe.borrow_mut().as_mut() {
        Some(found) => {
            *found = Some(url.to_owned());
            true
        }
        None => false,
    })
}

/// The link the pointer at `position` is on, as the kit's text view hit-tests
/// it. Only between events: the window's listeners are its own while one is
/// being handled. `position` is where the pointer is, as GPUI moves its idea
/// of the pointer to the event's. Nothing while the left button is down or
/// something's being dragged: GPUI would take the probe's release for theirs
/// (it ends a drag on any release).
pub(crate) fn link_at(position: Point<Pixels>, window: &mut Window, cx: &mut App) -> Option<String> {
    if PROBE.with(|p| p.borrow().is_some()) || LEFT_HELD.with(Cell::get) || cx.has_active_drag() {
        return None;
    }
    PROBE.with(|p| *p.borrow_mut() = Some(None));
    let event = MouseUpEvent { button: PROBE_BUTTON, position, modifiers: window.modifiers(), click_count: 1 };
    window.dispatch_event(event.to_platform_input(), cx);
    let found = PROBE.with(|p| p.borrow_mut().take().flatten());
    remember_probe(position, found.clone());
    found
}

/// Whether a click on a link is one that follows it: the left button, or the
/// middle one as a browser has it. A right click is the link's menu.
pub(crate) fn follows(event: &ClickEvent) -> bool {
    match event {
        ClickEvent::Mouse(click) => matches!(click.up.button, MouseButton::Left | MouseButton::Middle),
        _ => true,
    }
}

// ---------------------------------------------------------------- what a link names

/// Where a link in a reply goes, once it's been read.
#[derive(Clone, Debug, PartialEq)]
enum LinkTarget {
    Web(String),
    /// A file, if it could be found: a link to one, or a path in inline code.
    File(Option<FileLink>),
}

impl ThreadPane {
    /// What `url` names, for a click, a menu or a card. `work:`, `thread:`
    /// and Threadbear's other links name neither.
    fn link_target(&self, url: &str) -> Option<LinkTarget> {
        let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')));
        match scheme.as_deref() {
            Some("tbpath") => {
                let rest = &url["tbpath:".len()..];
                let text = crate::rows::markdown::percent_decode(rest.strip_prefix("?p=").unwrap_or(rest));
                Some(LinkTarget::File(self.find_file(&text)))
            }
            Some("work" | "thread" | "review" | "tbchip" | "button") => None,
            // `Foo.swift:12` reads as a scheme, but it's a file and a line.
            Some(s) if s.contains('.') && file_refs::strip_line(url) != url => Some(LinkTarget::File(self.find_file(url))),
            _ => match policy::link_kind(url) {
                LinkKind::Web(web) => Some(LinkTarget::Web(web)),
                // A link with no scheme is looked for from the thread's folder
                // like a path in inline code; a `file:` URL is the file.
                LinkKind::File(path) if path.starts_with('/') || threadbear_core::reducer::text::is_windows_path(&path) => Some(LinkTarget::File(Some(FileLink { path, query: None, fragment: None }))),
                LinkKind::File(path) => Some(LinkTarget::File(self.find_file(&path))),
                _ => None,
            },
        }
    }

    /// The file a path in a reply names: from the thread's folder and what
    /// its tools touched (`transcriptLinkFile`). Another Mac's paths are
    /// that Mac's, so nothing is looked for here: the path as written or
    /// from the thread's folder there, and its runner says whether it's
    /// there when it's read.
    fn find_file(&self, text: &str) -> Option<FileLink> {
        let local_home = threadbear_core::reducer::text::home_directory();
        let cwd = self.summary.cwd.clone();
        let root = (!cwd.is_empty()).then_some(cwd.as_str());
        if self.summary.is_remote() {
            let home = file_refs::remote_home(&cwd, &local_home);
            let known = file_refs::tool_paths(self.feed.model().items(), &home);
            return file_refs::file_link(text, root, &known, &home, &|p| p.starts_with('/'), &|_, _| None);
        }
        let known = file_refs::tool_paths(self.feed.model().items(), &local_home);
        file_refs::file_link(text, root, &known, &local_home, &|p| policy::local_metadata(Path::new(p)).is_ok(), &file_refs::search)
    }

    /// Whether the pane beside the transcript is this thread's: it is for
    /// the selected thread, but a sub-agent's transcript, a project's page
    /// or a thread in a panel of its own has none showing.
    fn owns_pane(&self, cx: &App) -> bool {
        let Some(app) = self.composer.read(cx).app.upgrade() else { return false };
        let app = app.read(cx);
        self.subagent_id.is_none() && app.selected.as_deref() == Some(self.summary.id.as_str()) && app.projects.open.is_none()
    }

    // ---------------------------------------------------------------- clicks

    /// A link in a reply clicked: `command` is ⌘ held (Control elsewhere).
    /// `work:`, `thread:` and `review:` links go somewhere in Threadbear;
    /// a file opens where Default says (⌘: its app); a web page in the
    /// browser or the pane; anything else through the open gate, which asks
    /// before another app starts.
    pub(crate) fn follow_link(&mut self, url: &str, row_id: &str, command: bool, window: &mut Window, cx: &mut Context<Self>) {
        let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
        let rest = url.split_once(':').map_or("", |(_, r)| r);
        match scheme.as_deref() {
            Some("work") => return self.open_work_link(&crate::rows::markdown::percent_decode(rest), row_id, window, cx),
            Some("thread") => return self.open_thread_link(&crate::rows::markdown::percent_decode(rest), window, cx),
            Some("review") => return self.open_review_link(&crate::rows::markdown::percent_decode(rest), window, cx),
            _ => {}
        }
        match self.link_target(url) {
            Some(LinkTarget::File(Some(file))) => self.open_file_link(file, None, command, cx),
            Some(LinkTarget::File(None)) => {
                let name = hl::last_component(&file_refs::strip_line(url).replace("tbpath:?p=", "")).to_owned();
                self.show_error(format!("Couldn't find {}.", crate::rows::markdown::percent_decode(&name)), cx);
            }
            Some(LinkTarget::Web(web)) => self.open_web_link(&web, WebLinkAction::for_click(command, web_click()), cx),
            None => open_gate::open_link(url, self.origin(), &self.summary.cwd, cx),
        }
    }

    /// A file opened as `action` says, or as a click does (`None`): ⌘ means
    /// its app, a project file goes to its app, else what Default says.
    fn open_file_link(&mut self, file: FileLink, action: Option<FileLinkAction>, command: bool, cx: &mut Context<Self>) {
        let path = file.path.clone();
        let remote = self.summary.is_remote();
        let clicked = action.is_none();
        let action = action.unwrap_or_else(|| FileLinkAction::for_click(command, remote, opens_in_its_app(&path), file_click()));
        // Another Mac's file isn't on this one: it's read into the pane
        // through its runner, whichever way it's opened, and isn't revealed.
        if remote {
            if action == FileLinkAction::Finder {
                open_gate::reveal(&path, Origin::OtherMac, cx);
            } else {
                self.preview_in_pane(&path, cx);
            }
            return;
        }
        let meta = match policy::local_metadata(Path::new(&path)) {
            Ok(meta) => meta,
            Err(error) => { self.show_error(format!("Couldn't open that file: {error}"), cx); return; }
        };
        let mut action = action.resolved(&path, meta.is_dir());
        // A plain click on a kind the pane can't draw yet (a PDF, a film)
        // goes to its app, rather than to a note saying it opens there.
        if clicked && action == FileLinkAction::Panel && !pane_draws(&path) && file_click() == FileLinkAction::Automatic {
            action = FileLinkAction::App;
        }
        match action {
            FileLinkAction::Panel if self.owns_pane(cx) => self.preview_in_pane(&path, cx),
            FileLinkAction::Finder => open_gate::reveal(&path, Origin::ThisMac, cx),
            // The pane belongs to the selected thread: a sub-agent's or a
            // project page's link goes to its app.
            _ => open_gate::open_file(&path, Origin::ThisMac, cx),
        }
    }

    /// A web page where `action` says: the browser, or the pane (only the
    /// selected thread's has one; elsewhere the browser).
    fn open_web_link(&mut self, url: &str, action: WebLinkAction, cx: &mut Context<Self>) {
        if action == WebLinkAction::Panel && self.owns_pane(cx) {
            let thread = self.summary.clone();
            if let Some(app) = self.composer.read(cx).app.upgrade() {
                let side = app.read(cx).side.clone();
                let section = side.update(cx, |side, cx| {
                    let section = side.section_for(&thread);
                    side.show_preview(PanePreview::new(Target::Page(url.to_owned()), hl::text::address(url), section.clone()), &thread.id, cx);
                    section
                });
                show_pane(&app, &section, cx);
            }
            return;
        }
        open_gate::open_link(url, self.origin(), &self.summary.cwd, cx);
    }

    /// A file in the pane beside the transcript, opening the pane if it's
    /// closed (`PaneModel.show`).
    fn preview_in_pane(&mut self, path: &str, cx: &mut Context<Self>) {
        let thread = self.summary.clone();
        let Some(app) = self.composer.read(cx).app.upgrade() else { return };
        let side = app.read(cx).side.clone();
        let section = side.update(cx, |side, cx| {
            side.preview_path(path, None, &thread, cx);
            side.section_for(&thread)
        });
        show_pane(&app, &section, cx);
    }
}

fn show_pane(app: &Entity<crate::app::Threadbear>, section: &str, cx: &mut App) {
    let section = section.to_owned();
    app.update(cx, |t, cx| {
        if !t.dock.read(cx).controller.is_showing(&section) {
            t.show_pane(&section, cx);
        }
    });
}

/// A project file with an app here to open it: a `.tandem` goes to Tandem
/// whatever Default says. The file itself counts, not the name a symlink
/// gives it (`FileLinkAction.opensInItsApp`); the gate looks again before
/// anything opens.
fn opens_in_its_app(path: &str) -> bool {
    if !hl::is_app_project(path) {
        return false;
    }
    let Ok(real) = threadbear_core::open_policy::real_path(Path::new(path)) else { return false };
    let real = real.to_string_lossy().into_owned();
    hl::is_app_project(&real) && open_gate::opening_app(&real).is_some()
}

/// Whether the pane draws the file itself: text, code, Markdown, pages and
/// pictures. PDFs and films open in their app until the pane has viewers
/// for them (`file.quicklook`, `file.video`).
fn pane_draws(path: &str) -> bool {
    match hl::viewer_for_path(path) {
        hl::Viewer::Text | hl::Viewer::Web => true,
        hl::Viewer::Picture => hl::looks_like_picture(path) || path.to_lowercase().ends_with(".svg"),
    }
}

// ---------------------------------------------------------------- the link's menu

/// A reply's text with what its links do on a right-click and under the
/// pointer: the link's own menu in place of the row's when the press is on
/// a link, and the link's card after the pointer rests on it.
/// `thread` is the pane's thread, handed in as the pane is drawing and can't
/// be read through its handle meanwhile.
pub(crate) fn link_aware(text: impl IntoElement, ix: usize, row: &Row, thread: &str, cx: &mut Context<ThreadPane>) -> AnyElement {
    let weak = cx.weak_entity();
    let thread = thread.to_owned();
    let row_id = row.id.clone();
    let (moved_pane, moved_thread, moved_row) = (weak.clone(), thread.clone(), row_id.clone());
    let (left_thread, left_row) = (thread.clone(), row_id.clone());
    let menu_row = row.clone();
    div()
        .id(("reply-links", ix))
        .on_mouse_move(move |event: &MouseMoveEvent, window, cx| pointer_moved(&moved_pane, &moved_thread, &moved_row, event, window, cx))
        .on_hover(move |hovered: &bool, _, cx| {
            if !*hovered {
                pointer_left(&left_thread, &left_row, cx);
            }
        })
        .on_scroll(|_, _, cx| hide_peek(cx))
        // A press is the start of a click or a selection, never a rest.
        .on_mouse_down(MouseButton::Left, |_, _, cx| hide_peek(cx))
        .context_menu(move |menu, window, cx| link_menu(menu, &weak, &menu_row, window, cx))
        .child(text)
        .into_any_element()
}

/// The menu for a right-click on a reply: the link's, if the press is on a
/// file or a web link (`fileLinkMenu(at:)`), else the row's own.
fn link_menu(menu: PopupMenu, pane: &WeakEntity<ThreadPane>, row: &Row, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    hide_peek(cx);
    let at = window.mouse_position();
    let url = recent_probe(at).unwrap_or_else(|| link_at(at, window, cx));
    let Some(entity) = pane.upgrade() else { return menu };
    let target = url.as_deref().and_then(|u| entity.read(cx).link_target(u));
    match target {
        None => {
            let open = entity.read(cx).row_open(row);
            ThreadPane::row_context_menu(row.clone(), menu, pane.clone(), open)
        }
        Some(LinkTarget::Web(url)) => {
            let profiles = offered_profiles();
            let entries = rules::web_link_menu(browser_name().as_deref(), &profiles, web_click());
            fill_menu(menu, &entries, &MenuTarget { pane: pane.clone(), link: LinkTarget::Web(url), profiles }, window, cx)
        }
        Some(LinkTarget::File(file)) => {
            let app = file.as_ref().filter(|_| !entity.read(cx).summary.is_remote()).and_then(|f| open_gate::opening_app(&f.path)).map(|a| a.name);
            let entries = rules::file_link_menu(file.is_some(), app.as_deref(), file_click());
            fill_menu(menu, &entries, &MenuTarget { pane: pane.clone(), link: LinkTarget::File(file), profiles: Vec::new() }, window, cx)
        }
    }
}

/// What a link menu's choices act on.
#[derive(Clone)]
struct MenuTarget {
    pane: WeakEntity<ThreadPane>,
    link: LinkTarget,
    profiles: Vec<ChromeProfile>,
}

fn fill_menu(mut menu: PopupMenu, entries: &[LinkMenuEntry], target: &MenuTarget, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    for entry in entries {
        match entry {
            LinkMenuEntry::Separator => menu = menu.separator(),
            LinkMenuEntry::Note { title, symbol } => menu = menu.item(PopupMenuItem::new(title.clone()).icon(crate::icons::symbol(symbol)).disabled(true)),
            LinkMenuEntry::Item { title, symbol, choice } => {
                let (target, choice) = (target.clone(), choice.clone());
                menu = menu.item(PopupMenuItem::new(title.clone()).icon(crate::icons::symbol(symbol)).on_click(move |_, _, cx| choose(&target, &choice, cx)));
            }
            LinkMenuEntry::Defaults { choices } => {
                let (choices, target) = (choices.clone(), target.clone());
                menu = menu.submenu_with_icon(Some(crate::icons::symbol(rules::DEFAULT_SYMBOL)), rules::DEFAULT_TITLE, window, cx, move |mut sub, _, _| {
                    for (title, symbol, choice, on) in &choices {
                        let (target, choice) = (target.clone(), choice.clone());
                        sub = sub.item(PopupMenuItem::new(title.clone()).icon(crate::icons::symbol(symbol)).checked(*on).on_click(move |_, _, cx| choose(&target, &choice, cx)));
                    }
                    sub
                });
            }
        }
    }
    menu
}

/// A link menu's choice made.
fn choose(target: &MenuTarget, choice: &LinkChoice, cx: &mut App) {
    match (choice, &target.link) {
        (LinkChoice::SetFileClick(action), _) => set_file_click(*action),
        (LinkChoice::SetWebClick(action), _) => set_web_click(*action),
        (LinkChoice::CopyPath, LinkTarget::File(Some(file))) => cx.write_to_clipboard(ClipboardItem::new_string(file.path.clone())),
        (LinkChoice::CopyLink, LinkTarget::Web(url)) => cx.write_to_clipboard(ClipboardItem::new_string(url.clone())),
        (LinkChoice::OpenInProfile(index), LinkTarget::Web(url)) => {
            if let Some(profile) = target.profiles.get(*index) {
                open_gate::open_in_chrome_profile(url, &profile.directory, cx);
            }
        }
        (LinkChoice::OpenWeb(action), LinkTarget::Web(url)) => {
            let (url, action) = (url.clone(), *action);
            let _ = target.pane.update(cx, |pane, cx| pane.open_web_link(&url, action, cx));
        }
        (LinkChoice::OpenFile(action), LinkTarget::File(Some(file))) => {
            let (file, action) = (file.clone(), *action);
            let _ = target.pane.update(cx, |pane, cx| pane.open_file_link(file, Some(action), false, cx));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- the hover card

/// The link the pointer is resting on, and its card once it's due.
struct Peek {
    /// Bumped by every move, so only the last rest asks.
    serial: u64,
    /// The reply the pointer's in (its thread and row), from its moves until
    /// it leaves: a rest that comes due once it's gone has nothing to ask.
    inside: Option<(String, String)>,
    hovered: Option<Hovered>,
    /// The last probe: where, what it found, and when, so a right-click
    /// where the pointer has rested needn't ask again.
    last: Option<(Point<Pixels>, Option<String>, Instant)>,
}

#[derive(Clone)]
struct Hovered {
    pane: WeakEntity<ThreadPane>,
    thread: String,
    row: String,
    url: String,
    /// Where it is on the line the pointer came to rest on.
    spot: Spot,
    card: Option<Card>,
}

/// Where a link sits on the line the pointer rests on: its left edge there,
/// kept so a card from it stays in the window, and the line's top and
/// bottom. Its card goes under that line, lined up with the link, or over
/// it when there's no room below, as ChipCardWindow places it.
#[derive(Clone, Copy, Debug)]
struct Spot {
    left: Pixels,
    top: Pixels,
    bottom: Pixels,
}

/// Where the link to `url` that the point `at` is on sits (`Spot`), found by
/// asking up, down and along its line, as the kit says where a link is only
/// by whether a point is on it. About twenty asks, once per card. The last
/// is where the pointer is, as each moves GPUI's idea of the pointer there.
fn link_spot(at: Point<Pixels>, url: &str, window: &mut Window, cx: &mut App) -> Spot {
    let line = gpui_kit::px(style::zoomed(size::BODY_LINE));
    let top = last_on(url, at, point(at.x, at.y - line), window, cx).y;
    let bottom = last_on(url, at, point(at.x, at.y + line), window, cx).y;
    let reach = gpui_kit::px(style::zoomed(480.));
    let left = last_on(url, at, point((at.x - reach).max(Pixels::ZERO), at.y), window, cx).x;
    link_at(at, window, cx);
    // In the window, as the card's 300 wide (4 from the screen's edges on the Mac).
    let margin = gpui_kit::px(style::zoomed(4.));
    let furthest = window.viewport_size().width - gpui_kit::px(style::zoomed(300.)) - margin;
    Spot { left: left.min(furthest).max(margin), top, bottom }
}

/// The last point from `from`, which is on the link, towards `to` that's
/// still on it: a link is one run on its line, so halving the gap finds
/// its edge to a pixel.
fn last_on(url: &str, from: Point<Pixels>, to: Point<Pixels>, window: &mut Window, cx: &mut App) -> Point<Pixels> {
    let on = |p: Point<Pixels>, window: &mut Window, cx: &mut App| link_at(p, window, cx).as_deref() == Some(url);
    if on(to, window, cx) {
        return to;
    }
    let (mut inside, mut outside) = (from, to);
    while (outside - inside).magnitude() > 1. {
        let middle = point((inside.x + outside.x) / 2., (inside.y + outside.y) / 2.);
        if on(middle, window, cx) {
            inside = middle;
        } else {
            outside = middle;
        }
    }
    inside
}

thread_local! {
    static PEEK: RefCell<Peek> = const { RefCell::new(Peek { serial: 0, inside: None, hovered: None, last: None }) };
}

fn remember_probe(at: Point<Pixels>, found: Option<String>) {
    PEEK.with(|p| p.borrow_mut().last = Some((at, found, Instant::now())));
}

/// What the last probe found here, if it was here and just now.
fn recent_probe(at: Point<Pixels>) -> Option<Option<String>> {
    PEEK.with(|p| {
        let p = p.borrow();
        let (where_, found, when) = p.last.as_ref()?;
        ((*where_ - at).magnitude() < 0.5 && when.elapsed() < Duration::from_secs(5)).then(|| found.clone())
    })
}

fn pointer_moved(pane: &WeakEntity<ThreadPane>, thread: &str, row: &str, event: &MouseMoveEvent, window: &mut Window, cx: &mut App) {
    let serial = PEEK.with(|p| {
        let mut p = p.borrow_mut();
        p.serial += 1;
        p.inside = Some((thread.to_owned(), row.to_owned()));
        p.serial
    });
    if event.pressed_button.is_some() {
        hide_peek(cx);
        return;
    }
    let (pane, thread, row) = (pane.clone(), thread.to_owned(), row.to_owned());
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(REST).await;
            let _ = cx.update(|window, cx| rested(serial, &pane, &thread, &row, window, cx));
        })
        .detach();
}

/// The pointer has stopped: which link it's on, and the card's timer if
/// it's a new one.
fn rested(serial: u64, pane: &WeakEntity<ThreadPane>, thread: &str, row: &str, window: &mut Window, cx: &mut App) {
    let due = PEEK.with(|p| {
        let p = p.borrow();
        p.serial == serial && p.inside.as_ref().is_some_and(|(t, r)| t == thread && r == row)
    });
    if !due {
        return;
    }
    let at = window.mouse_position();
    let Some(url) = link_at(at, window, cx).filter(|u| has_card(u)) else { return hide_peek(cx) };
    // Still on the same link: its timer or its card stands. Which file a
    // path names is only worked out once its card is due, as that can mean
    // a look through folders.
    let same = PEEK.with(|p| p.borrow().hovered.as_ref().is_some_and(|h| h.url == url && h.row == row && h.thread == thread));
    if same {
        return;
    }
    hide_peek(cx);
    let spot = link_spot(at, &url, window, cx);
    PEEK.with(|p| p.borrow_mut().hovered = Some(Hovered { pane: pane.clone(), thread: thread.to_owned(), row: row.to_owned(), url: url.clone(), spot, card: None }));
    let (pane, row, thread) = (pane.clone(), row.to_owned(), thread.to_owned());
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(PEEK_AFTER.saturating_sub(REST)).await;
            let still = PEEK.with(|p| p.borrow().hovered.as_ref().is_some_and(|h| h.url == url && h.row == row && h.thread == thread));
            if !still {
                return;
            }
            let Some(card) = pane.read_with(cx, |pane, _| pane.peek_card(&url, &row)).ok().flatten() else { return };
            let card = card.load(cx.background_executor().clone()).await;
            PEEK.with(|p| {
                if let Some(h) = p.borrow_mut().hovered.as_mut().filter(|h| h.url == url && h.row == row) {
                    h.card = Some(card);
                }
            });
            let _ = pane.update(cx, |_, cx| cx.notify());
        })
        .detach();
}

/// Whether a link can have a card, cheaply: a web page or a file does
/// (`peek(atCharacter:)`); Threadbear's own links (a step, a thread, a
/// review, a chip, a reply button) and other apps' (`mailto:`) don't.
pub(crate) fn has_card(url: &str) -> bool {
    match url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()) {
        Some(s) if s == "http" || s == "https" || s == "file" || s == "tbpath" => true,
        // `Foo.swift:12`: a file and a line.
        Some(s) if s.contains('.') => file_refs::strip_line(url) != url,
        Some(s) if s.len() == 1 => true,
        Some(_) => false,
        None => !url.starts_with('#'),
    }
}

/// The pointer has left a reply's words: its card goes, and a rest still to
/// come there asks nothing. Only this reply's: moving straight into the next
/// one, that one's move can come first.
fn pointer_left(thread: &str, row: &str, cx: &mut App) {
    let here = |t: &str, r: &str| t == thread && r == row;
    let showing = PEEK.with(|p| {
        let mut p = p.borrow_mut();
        if p.inside.as_ref().is_some_and(|(t, r)| here(t, r)) {
            p.inside = None;
        }
        p.hovered.as_ref().is_some_and(|h| here(&h.thread, &h.row))
    });
    if showing {
        hide_peek(cx);
    }
}

/// Puts any card away, and lets a rest that's still to come go: whatever
/// put it away (a press, a scroll, the pointer leaving, a menu) means the
/// pointer isn't resting on a link any more.
pub(crate) fn hide_peek(cx: &mut App) {
    PEEK.with(|p| p.borrow_mut().serial += 1);
    let Some(gone) = PEEK.with(|p| p.borrow_mut().hovered.take()) else { return };
    if gone.card.is_some() {
        let _ = gone.pane.update(cx, |_, cx| cx.notify());
    }
}

/// What a link's card shows (LinkPeekCard).
#[derive(Clone)]
enum Card {
    Page { title: String, detail: String },
    File { words: rules::PeekWords, picture: FilePicture },
}

#[derive(Clone)]
enum FilePicture {
    Image(ImageSource),
    /// Its first lines, read once the card's due.
    Lines(PathBuf, Option<Vec<String>>),
    Symbol(&'static str),
}

impl Card {
    /// A text file's first lines, read off the main thread: a file on a
    /// slow disk mustn't hold up the window.
    async fn load(self, executor: BackgroundExecutor) -> Card {
        match self {
            Card::File { words, picture: FilePicture::Lines(path, None) } => {
                let read = path.clone();
                let lines = executor.spawn(async move { first_lines_of(&read) }).await;
                Card::File { words, picture: FilePicture::Lines(path, lines) }
            }
            other => other,
        }
    }
}

/// The start of a file, as its card's lines.
fn first_lines_of(path: &Path) -> Option<Vec<String>> {
    let sample = policy::local_read(path, 4096).ok()?;
    rules::first_lines(&sample, 10, 70)
}

impl ThreadPane {
    /// The card for the link `url` in `row`, or None for one with no card:
    /// another Mac's file, which isn't here to look at.
    fn peek_card(&self, url: &str, row: &str) -> Option<Card> {
        match self.link_target(url)? {
            LinkTarget::Web(web) => {
                let words = self.link_words(row, url).unwrap_or_default();
                let words = rules::page_peek(&web, &words);
                Some(Card::Page { title: words.title, detail: words.detail })
            }
            LinkTarget::File(_) if self.summary.is_remote() => None,
            LinkTarget::File(None) => None,
            LinkTarget::File(Some(file)) => {
                let meta = policy::local_metadata(Path::new(&file.path)).ok();
                let exists = meta.is_some();
                let words = rules::file_peek(&file.path, exists, None, &crate::files::home_dir());
                let picture = match (exists, rules::peek_picture(&file.path)) {
                    (true, PeekPicture::Image) => FilePicture::Image(crate::rows::markdown::file_picture(&file.path)),
                    (true, PeekPicture::Lines) => FilePicture::Lines(PathBuf::from(&file.path), None),
                    _ => FilePicture::Symbol(if meta.is_some_and(|m| m.is_dir()) { "folder" } else { hl::file_symbol(&file.path) }),
                };
                Some(Card::File { words, picture })
            }
        }
    }

    /// A Needs you card's link button's card (the guide's "in a reply or on
    /// a Needs you card's buttons"): what `peek_card` shows for a reply's
    /// link, with the button's words for a page's title. Drawn at once, so
    /// a text file shows its symbol rather than its first lines, which are
    /// only read off the main thread. None for a link with no card (a
    /// review, a thread, another Mac's file).
    pub(crate) fn ask_link_card(&self, title: &str, url: &str, cx: &App) -> Option<AnyElement> {
        if !has_card(url) {
            return None;
        }
        Some(match self.peek_card_named(url, title)? {
            Card::Page { title, detail } => crate::chips::words_card(&title, &detail, cx),
            Card::File { words, picture } => file_card(&words, &picture, cx),
        })
    }

    /// The card's title and detail, for the dump (`peek` on a card's link).
    #[cfg(feature = "automation")]
    pub(crate) fn ask_link_words(&self, title: &str, url: &str) -> Option<(String, String)> {
        has_card(url).then_some(())?;
        Some(match self.peek_card_named(url, title)? {
            Card::Page { title, detail } => (title, detail),
            Card::File { words, .. } => (words.title, words.detail),
        })
    }

    fn peek_card_named(&self, url: &str, words: &str) -> Option<Card> {
        match self.link_target(url)? {
            LinkTarget::Web(web) => {
                let words = rules::page_peek(&web, words);
                Some(Card::Page { title: words.title, detail: words.detail })
            }
            LinkTarget::File(_) if self.summary.is_remote() => None,
            LinkTarget::File(None) => None,
            LinkTarget::File(Some(file)) => {
                let meta = policy::local_metadata(Path::new(&file.path)).ok();
                let exists = meta.is_some();
                let words = rules::file_peek(&file.path, exists, None, &crate::files::home_dir());
                let picture = match (exists, rules::peek_picture(&file.path)) {
                    (true, PeekPicture::Image) => FilePicture::Image(crate::rows::markdown::file_picture(&file.path)),
                    _ => FilePicture::Symbol(if meta.is_some_and(|m| m.is_dir()) { "folder" } else { hl::file_symbol(&file.path) }),
                };
                Some(Card::File { words, picture })
            }
        }
    }

    /// A link's words in a row's reply, for its card's title.
    fn link_words(&self, row: &str, url: &str) -> Option<String> {
        let row = self.feed.model().rows().iter().find(|r| r.id == row)?;
        let source = match &row.content {
            RowContent::Assistant(item) => crate::rows::markdown::prepare(&item.text),
            RowContent::User(message) => message.agent_header()?.body,
            _ => return None,
        };
        reply_links_in(&source).into_iter().find(|(_, u)| u == url).map(|(words, _)| words)
    }
}

/// Every link in a reply's markdown as the reply draws it, in order: its
/// words and where it goes.
fn reply_links_in(source: &str) -> Vec<(String, String)> {
    fn walk(node: &markdown::mdast::Node, out: &mut Vec<(String, String)>) {
        if let markdown::mdast::Node::Link(link) = node {
            out.push((node.to_string(), link.url.clone()));
            return;
        }
        for child in node.children().into_iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    if let Ok(tree) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) {
        walk(&tree, &mut out);
    }
    out
}

/// The card under the link the pointer's resting on, if this pane's.
pub(crate) fn peek_overlay(pane: &ThreadPane, cx: &App) -> Option<AnyElement> {
    let (card, spot) = PEEK.with(|p| p.borrow().hovered.as_ref().filter(|h| h.thread == pane.summary.id).and_then(|h| h.card.clone().map(|c| (c, h.spot))))?;
    let card = match card {
        Card::Page { title, detail } => crate::chips::words_card(&title, &detail, cx),
        Card::File { words, picture } => file_card(&words, &picture, cx),
    };
    // 4 under the link's line, lined up with it, or 4 over the line when
    // there's no room below (ChipCardWindow): `anchored` turns the card over
    // about its position, without the offset, so the position is over the
    // line and the offset takes it under. Hover only: it takes no clicks,
    // so the link under it is still what's there.
    let gap = gpui_kit::px(style::zoomed(4.));
    let under = point(Pixels::ZERO, spot.bottom - spot.top + gap * 2.);
    Some(deferred(anchored().position(point(spot.left, spot.top - gap)).offset(under).child(card)).with_priority(2).into_any_element())
}

/// A file's card (LinkPeekCard): its picture or first lines over its name,
/// or its symbol beside it, its folder, and what a click does.
fn file_card(words: &rules::PeekWords, picture: &FilePicture, cx: &App) -> AnyElement {
    const BOX: (f32, f32) = (276., 170.);
    let (secondary, tertiary, label) = (style::secondary_label(cx), style::tertiary_label(cx), style::label_text(cx));
    let picture_box = || div().w(px(BOX.0)).h(px(BOX.1)).rounded(px(4.)).bg(style::primary_opacity(cx, 0.05)).overflow_hidden();
    let shown = match picture {
        FilePicture::Image(path) => Some(picture_box().flex().items_center().justify_center().child(img(path.clone()).max_w(px(BOX.0)).max_h(px(BOX.1)).rounded(px(3.)).object_fit(ObjectFit::Contain)).into_any_element()),
        FilePicture::Lines(_, Some(lines)) => Some(
            picture_box()
                .p(px(8.))
                .child(v_flex().children(lines.iter().map(|line| div().text_size(px(10.)).line_height(px(14.)).font_family(cx.theme().mono_font_family.clone()).text_color(secondary).whitespace_nowrap().child(if line.is_empty() { " ".to_owned() } else { line.clone() }))))
                .into_any_element(),
        ),
        FilePicture::Lines(_, None) => Some(picture_box().into_any_element()),
        FilePicture::Symbol(_) => None,
    };
    let symbol = match picture {
        FilePicture::Symbol(name) => Some(div().flex_none().size(px(28.)).flex().items_center().justify_center().child(crate::icons::symbol(name).size(px(22.)).text_color(secondary))),
        _ => None,
    };
    crate::chips::card_frame(cx)
        .w(px(300.))
        .px(px(12.))
        .py(px(10.))
        .child(
            v_flex()
                .gap(px(3.))
                .when_some(shown, |d, picture| d.child(div().pb(px(5.)).child(picture)))
                .child(
                    h_flex()
                        .items_start()
                        .gap(px(8.))
                        .children(symbol)
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap(px(3.))
                                .child(div().text_size(px(size::LABEL)).line_height(px(style::natural_line(size::LABEL))).font_weight(FontWeight::SEMIBOLD).text_color(label).line_clamp(2).child(words.title.clone()))
                                .child(div().text_size(px(size::DETAIL)).line_height(px(style::natural_line(size::DETAIL))).text_color(secondary).truncate().child(words.detail.clone())),
                        ),
                )
                .child(div().pt(px(3.)).text_size(px(size::SMALL)).line_height(px(style::natural_line(size::SMALL))).text_color(tertiary).child(words.footer.clone())),
        )
        .into_any_element()
}

// ---------------------------------------------------------------- scripts

/// `peek` on a reply (the Swift app's, for its links): `{"row": id}` lists
/// the links with a card ("words=url | words=/path"); with `words` (part of
/// a link's words), shows that link's card as resting on it would ("ok
/// window 0") and `show: false` puts it away; `menu: true` lists its
/// right-click menu, `click` runs one of its items and `default` picks one
/// under its Default; `follow: true` clicks it through real pointer events,
/// with ⌘ held for `"command": true`.
#[cfg(feature = "automation")]
pub(crate) fn peek_command(app: &Entity<crate::app::Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Option<Result<serde_json::Value, String>> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    use serde_json::json;
    let pane = app.read(cx).pane.clone()?;
    let prefix = cmd["row"].as_str()?;
    let (ix, row) = {
        let rows = pane.read(cx).feed.model().rows();
        let ix = rows.iter().position(|r| r.id.starts_with(prefix))?;
        (ix, rows[ix].clone())
    };
    let RowContent::Assistant(item) = &row.content else { return None };
    if cmd["show"].as_bool() == Some(false) {
        hide_peek(cx);
        return Some(Ok(json!("ok")));
    }
    let links = reply_links_in(&crate::rows::markdown::prepare(&item.text));
    let named = |url: &str| -> Option<String> {
        match pane.read(cx).link_target(url)? {
            LinkTarget::Web(web) => Some(web),
            LinkTarget::File(Some(file)) if !pane.read(cx).summary.is_remote() && policy::local_metadata(Path::new(&file.path)).is_ok() => Some(file.path),
            LinkTarget::File(_) => None,
        }
    };
    let Some(words) = cmd["words"].as_str() else {
        let listed: Vec<String> = links.iter().filter_map(|(w, u)| named(u).map(|to| format!("{}={to}", w.trim()))).collect();
        return Some(Ok(json!(if listed.is_empty() { "ok none".to_owned() } else { format!("ok {}", listed.join(" | ")) })));
    };
    let Some((_, url)) = links.iter().find(|(w, _)| w.contains(words)).cloned() else { return Some(Err(format!("no link with {words} in {}", row.id))) };
    // On screen first, as a person would see it, then where the kit says
    // the link is.
    if let Err(e) = pane.update(cx, |p, cx| p.scroll_command(None, Some(&row.id), 120., cx)) {
        return Some(Err(e));
    }
    window.render_frame(cx);
    let Some(at) = find_link(&url, ix, words, window, cx) else {
        // What was there instead, for a script to say why.
        let mine = ElementId::NamedInteger("reply-links".into(), ix as u64);
        let texts = gpui_kit::base::test_support::texts(window).into_iter().filter(|t| t.visible() && t.path().contains(&mine)).map(|t| t.text().to_owned()).collect::<Vec<_>>();
        return Some(Err(format!("the link {words} isn't on screen (its row shows {texts:?})")));
    };
    if cmd["menu"].as_bool() == Some(true) || cmd.get("click").is_some() || cmd.get("default").is_some() {
        // A right-click through real events and hit testing, as a person's
        // goes. The kit builds the menu once the press has been handled,
        // which is after this command's turn: `menu_command` reads it then.
        hide_peek(cx);
        window.pointer_move(at, None, cx);
        window.pointer_down(at, MouseButton::Right, cx);
        window.pointer_up(at, MouseButton::Right, cx);
        return Some(Ok(json!("ok right-clicked")));
    }
    if cmd["follow"].as_bool() == Some(true) {
        let modifiers = if cmd["command"].as_bool() == Some(true) { Modifiers::secondary_key(cx) } else { Modifiers::default() };
        window.pointer_move(at, None, cx);
        window.dispatch_event(MouseDownEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1, first_mouse: false }.to_platform_input(), cx);
        window.dispatch_event(MouseUpEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1 }.to_platform_input(), cx);
        window.render_frame(cx);
        return Some(Ok(json!("ok")));
    }
    // Its card, as resting the pointer there shows it.
    hide_peek(cx);
    let Some(card) = pane.read(cx).peek_card(&url, &row.id) else { return Some(Err(format!("{words} has no card"))) };
    let card = match card {
        Card::File { words, picture: FilePicture::Lines(path, None) } => {
            let lines = first_lines_of(&path);
            Card::File { words, picture: FilePicture::Lines(path, lines) }
        }
        other => other,
    };
    let spot = link_spot(at, &url, window, cx);
    PEEK.with(|p| p.borrow_mut().hovered = Some(Hovered { pane: pane.downgrade(), thread: pane.read(cx).summary.id.clone(), row: row.id.clone(), url, spot, card: Some(card) }));
    pane.update(cx, |_, cx| cx.notify());
    window.render_frame(cx);
    Some(Ok(json!("ok window 0")))
}

/// `peek` with `menu`, `click` or `default`: the right-click (`peek_command`),
/// then, once the kit has built the menu, it read back; `click` presses one
/// of its items and `default` one of Default's, through real events, a
/// command's turn each, as each waits on the one before to be drawn.
#[cfg(feature = "automation")]
pub(crate) async fn menu_command(app: &WeakEntity<crate::app::Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    use serde_json::json;
    let app = app.upgrade().ok_or("the app has gone")?;
    let words = cmd["words"].as_str().unwrap_or("").to_owned();
    let clicked = cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| peek_command(&app, cmd, window, cx))).map_err(|e| e.to_string())?;
    match clicked {
        Some(Ok(_)) => {}
        Some(Err(e)) => return Err(e),
        None => return Err("that row has no links of a reply's".into()),
    }
    let settle = Duration::from_millis(150);
    cx.background_executor().timer(settle).await;
    let read = |nth: usize, cx: &mut AsyncApp| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            open_menu_items(window, nth)
        })
        .map_err(|e| e.to_string())
    };
    let press = |at: Point<Pixels>, cx: &mut AsyncApp| {
        cx.update_window(window, |_, window, cx| {
            crate::thread::scripted_input(|| {
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
            })
        })
        .map_err(|e| e.to_string())
    };
    let items = read(0, cx)?;
    if items.is_empty() {
        return Err(format!("no menu came up on {words}"));
    }
    if let Some(title) = cmd["default"].as_str() {
        let Some((_, default)) = items.iter().find(|(t, _)| t == rules::DEFAULT_TITLE) else { return Err("no Default in that menu".into()) };
        // Its choices open beside it once the pointer's on it.
        let at = default.center();
        cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_move(at, None, cx))).map_err(|e| e.to_string())?;
        cx.background_executor().timer(Duration::from_millis(400)).await;
        let choices = read(1, cx)?;
        let Some((_, choice)) = choices.iter().find(|(t, _)| t == title) else { return Err(format!("no Default item {title} in {choices:?}")) };
        press(choice.center(), cx)?;
        return Ok(json!("ok"));
    }
    if let Some(title) = cmd["click"].as_str() {
        let Some((_, item)) = items.iter().find(|(t, _)| t == title) else { return Err(format!("no item {title} in {items:?}")) };
        press(item.center(), cx)?;
        return Ok(json!("ok"));
    }
    let titles: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
    let listed = json!(format!("ok {}", serde_json::to_string(&titles).unwrap_or_default()));
    // Left up for a picture with `keep: true`; put away otherwise.
    if cmd["keep"].as_bool() != Some(true) {
        cx.update_window(window, |_, window, cx| window.press("escape", cx)).map_err(|e| e.to_string())?;
    }
    Ok(listed)
}

/// The items of the `nth` menu on screen (0 the first opened, 1 a submenu
/// beside it), by their words and where they are, top to bottom.
#[cfg(feature = "automation")]
pub(crate) fn open_menu_items(window: &mut Window, nth: usize) -> Vec<(String, Bounds<Pixels>)> {
    use gpui_kit::test::TestQueryExt as _;
    let elements = window.elements();
    let mut menus: Vec<_> = elements.iter().filter(|e| e.visible() && e.role() == Some(Role::Menu)).collect();
    menus.sort_by(|a, b| a.bounds().origin.x.partial_cmp(&b.bounds().origin.x).unwrap_or(std::cmp::Ordering::Equal));
    let Some(menu) = menus.get(nth) else { return Vec::new() };
    let path = menu.path().to_vec();
    elements
        .iter()
        .filter(|e| e.visible() && e.path().len() > path.len() && e.path().starts_with(&path) && e.role() != Some(Role::Menu))
        .filter_map(|e| e.label().map(|label| (label.to_owned(), e.bounds())))
        .collect()
}

/// Where on screen the link to `url` in the `ix`-th row is: the kit's text
/// for that row says which paragraph holds the words, the words shaped as
/// the kit shapes them say about where in it, and asking there finds the
/// link itself, as only the kit knows where it put each word. Each ask goes
/// through the whole window, so it asks a few times, not along every line.
#[cfg(feature = "automation")]
fn find_link(url: &str, ix: usize, words: &str, window: &mut Window, cx: &mut App) -> Option<Point<Pixels>> {
    use gpui_kit::test::TestEventExt as _;
    let mine = ElementId::NamedInteger("reply-links".into(), ix as u64);
    let words = words.trim();
    let texts: Vec<(String, Bounds<Pixels>)> = gpui_kit::base::test_support::texts(window).into_iter().filter(|t| t.visible() && t.path().contains(&mine) && t.text().contains(words)).map(|t| (t.text().to_owned(), t.bounds())).collect();
    let line = gpui_kit::px(style::zoomed(size::BODY_LINE));
    // After a key (a script's Escape closing a menu), GPUI counts nothing
    // as under the pointer until it moves, and the probe is a release, which
    // doesn't count as moving. A person's hand goes back to the mouse first.
    window.pointer_move(window.mouse_position(), None, cx);
    let ask = |at: Point<Pixels>, window: &mut Window, cx: &mut App| link_at(at, window, cx).as_deref() == Some(url);
    for (text, bounds) in &texts {
        if let Some(guess) = shaped_middle(text, words, bounds, window) {
            let nudge = gpui_kit::px(style::zoomed(6.));
            for (dx, dy) in [(0., 0.), (-1., 0.), (1., 0.), (-2., 0.), (2., 0.), (0., -1.), (0., 1.)] {
                let at = point(guess.x + nudge * dx, guess.y + line * dy);
                if bounds.contains(&at) && ask(at, window, cx) {
                    return Some(at);
                }
            }
        }
    }
    // The guess missed (a bold word or a code span shapes wider): along
    // each line, a link's width at a time.
    let step = gpui_kit::px(style::zoomed(10.));
    for (_, bounds) in &texts {
        let mut y = bounds.top() + line / 2.;
        while y < bounds.bottom() {
            let mut x = bounds.left() + step / 2.;
            while x < bounds.right() {
                if ask(point(x, y), window, cx) {
                    return Some(point(x, y));
                }
                x += step;
            }
            y += line;
        }
    }
    None
}

/// Where the middle of `words` would be in a paragraph of `text` laid out in
/// `bounds`, shaped in the reply's font and size and wrapped at its width.
#[cfg(feature = "automation")]
fn shaped_middle(text: &str, words: &str, bounds: &Bounds<Pixels>, window: &Window) -> Option<Point<Pixels>> {
    let start = text.find(words)?;
    let mut middle = start + words.len() / 2;
    while !text.is_char_boundary(middle) {
        middle -= 1;
    }
    let line = gpui_kit::px(style::zoomed(size::BODY_LINE));
    let style = window.text_style();
    let mut top = Pixels::ZERO;
    let mut from = 0;
    for hard in text.split('\n') {
        let end = from + hard.len();
        if middle <= end {
            let run = TextRun { len: hard.len().max(1), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
            let shaped = window.text_system().shape_text(SharedString::from(if hard.is_empty() { " ".to_owned() } else { hard.to_owned() }), gpui_kit::px(style::zoomed(size::BODY)), &[run], Some(bounds.size.width), None).ok()?;
            let at = shaped.first()?.position_for_index(middle - from, line)?;
            return Some(point(bounds.left() + at.x, bounds.top() + top + at.y + line / 2.));
        }
        // A hard line's height, shaped the same way.
        let run = TextRun { len: hard.len().max(1), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
        let shaped = window.text_system().shape_text(SharedString::from(if hard.is_empty() { " ".to_owned() } else { hard.to_owned() }), gpui_kit::px(style::zoomed(size::BODY)), &[run], Some(bounds.size.width), None).ok()?;
        top += line * (shaped.first().map_or(0, |l| l.wrap_boundaries.len()) + 1) as f32;
        from = end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{has_card, reply_links_in};

    #[test]
    fn pages_and_files_have_cards_and_threadbears_own_links_dont() {
        for url in ["https://x.dev/a", "http://x.dev", "file:///tmp/a.md", "tbpath:?p=src/a.rs", "/tmp/a.md", "docs/plan.md", "Foo.swift:12", "~/notes.md"] {
            assert!(has_card(url), "{url}");
        }
        for url in ["work:npm%20test", "thread:4f9c2e7a", "review:r1", "tbchip:abc", "button:b7k2", "mailto:a@b.c", "slack://open", "#usage"] {
            assert!(!has_card(url), "{url}");
        }
    }

    #[test]
    fn a_replys_links_are_found_with_their_words_in_order() {
        let links = reply_links_in("See [the **notes**](https://x.dev/n) and [`a.rs`](tbpath:?p=a.rs), not `code`.");
        assert_eq!(links, [("the notes".to_owned(), "https://x.dev/n".to_owned()), ("a.rs".to_owned(), "tbpath:?p=a.rs".to_owned())]);
        assert!(reply_links_in("```\n[x](https://x.dev)\n```").is_empty(), "a link in a code block is code");
    }
}

/// The probe against the gate's own text view (the handler every reply link
/// goes through) and a real click target, in a window of the test platform's,
/// which opens nothing (the kit's test support comes with the `automation`
/// feature).
#[cfg(all(test, feature = "automation"))]
mod probe_tests {
    use super::{link_at, link_spot, watch};
    use crate::open_gate::{self, Origin};
    use gpui_kit::base::TextViewState;
    use gpui_kit::prelude::*;
    use gpui_kit::test::TestEventExt as _;
    // By name: a glob of the kit would bring its `test` over the one the
    // test macro's own expansion uses.
    use gpui_kit::{div, point, px, size, Bounds, Entity, MouseButton, Point, TestAppContext, Window, WindowBounds, WindowOptions};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Reply {
        text: Entity<TextViewState>,
        pressed: Rc<Cell<u32>>,
    }

    impl Render for Reply {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let pressed = self.pressed.clone();
            div()
                .size_full()
                .child(watch())
                .child(div().w(px(240.)).child(open_gate::text_view(&self.text, Origin::ThisMac, "")))
                .child(div().id("button").absolute().left(px(0.)).top(px(200.)).w(px(100.)).h(px(30.)).on_click(move |_, _, _| pressed.set(pressed.get() + 1)))
        }
    }

    #[gpui_kit::test]
    fn a_probe_finds_the_link_and_never_clicks_what_the_left_button_holds(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let pressed = Rc::new(Cell::new(0));
        let in_view = pressed.clone();
        let window = cx.update(|cx| {
            let bounds = Bounds { origin: Point::default(), size: size(px(400.), px(300.)) };
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
            gpui_kit::open_window(options, cx, move |_, cx| cx.new(|cx| Reply { text: cx.new(|cx| TextViewState::markdown("See [example](https://example.com) now.", cx)), pressed: in_view })).expect("a test window").0
        });
        let url = "https://example.com";
        cx.update_window(window, |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            assert_eq!(link_at(point(px(1.), px(10.)), window, cx), None, "\"See\" isn't the link");
            let on_link = (1..120).map(|x| point(px(x as f32 * 2.), px(10.))).find(|&p| link_at(p, window, cx).as_deref() == Some(url)).expect("the link along its line");
            // Where it sits, for its card: its start, and its line's top and bottom.
            let spot = link_spot(on_link, url, window, cx);
            assert!(spot.left > px(4.) && spot.left <= on_link.x, "{spot:?} from {on_link:?}");
            assert_eq!(link_at(point(spot.left + px(1.), on_link.y), window, cx).as_deref(), Some(url));
            assert_ne!(link_at(point(spot.left - px(2.), on_link.y), window, cx).as_deref(), Some(url));
            assert!(spot.top < on_link.y && on_link.y < spot.bottom && spot.bottom - spot.top < px(40.), "{spot:?}");
            let button = point(px(50.), px(215.));
            assert_eq!(link_at(button, window, cx), None, "no link there");
            assert_eq!(pressed.get(), 0, "a probe's release isn't a click");
            // Pressed and held, as a quick click on a chip is when the
            // pointer's rest comes due: a probe would be the release.
            window.pointer_move(button, None, cx);
            window.pointer_down(button, MouseButton::Left, cx);
            assert_eq!(link_at(button, window, cx), None);
            assert_eq!(pressed.get(), 0, "the probe clicked the held button early");
            window.pointer_up(button, MouseButton::Left, cx);
            assert_eq!(pressed.get(), 1, "the real release is the click");
            // Let go, it asks again.
            assert_eq!(link_at(on_link, window, cx).as_deref(), Some(url));
        })
        .expect("the test window");
        // Every one of those asks landed on the gate's handler, which noted
        // the link and followed nothing.
        assert_eq!(cx.opened_url(), None);
    }
}
