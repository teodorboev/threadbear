// Web panels beside a thread (panel.web, panel.bar, panel.convex): a patch's
// url panel and the Convex panel, ports of the Swift app's WebPanel,
// WebPanelPool, DockPaneButtons, ConvexPanel and ConvexWebPage
// (PanelsView.swift), drawn through the web runtime (web_runtime.rs).
//
// A page is a native view (threadbear_web::PanelPage) over its panel's
// layout slot, kept loaded between visits, four at a time as the Swift pool
// keeps them, the one used longest ago closing first. GPUI can't draw over a
// native view, so a page is shown and placed by the main window once a frame,
// after everything's laid out (`sweep`): where its panel was drawn that
// frame, hidden everywhere else, and hidden while a dialog, a sheet or one of
// the app's menus could be over it. For the same reason the Convex panel's
// deployment menu is a native one, as the Swift app's is anyway.
//
// Until the runtime's installed (or where there's none yet), a panel asks for
// it as diagrams do, says why when it can't, and offers Open in browser, so
// the app works without it.
use crate::app::Threadbear;
use crate::highlights::{icon, palette, tipped, Palette};
use crate::push_button::PushButton;
use crate::style::{self, px};
use crate::web_runtime::{self, Engine};
use futures::StreamExt as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use threadbear_client::runner::Client;
use threadbear_core::convex_panel::{self as convex, Choice, Deployment, Key, Login};
use threadbear_core::highlights::Panel;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::web_runtime::State;
use threadbear_web::{PanelEvent, PanelKind, PanelPage};

/// Pages kept loaded at once (WebPanelPool.capacity).
const CAPACITY: usize = 4;

thread_local! {
    /// The main window's frames, counted as it starts each: a page placed in
    /// this frame is shown, one that wasn't is hidden (`sweep`).
    static FRAME: Cell<u64> = const { Cell::new(0) };
}

fn frame_now() -> u64 {
    FRAME.with(Cell::get)
}

/// A page in the pool.
struct Kept {
    /// A patch's url, or "convex:<deployment>".
    key: String,
    page: Rc<PanelPage>,
    /// The frame it was last placed in, and where.
    drawn: Rc<Cell<(u64, Bounds<Pixels>)>>,
    _events: Task<()>,
}

/// What a Convex panel holds (ConvexPanel's state), by "<thread>/<panel>".
#[derive(Default)]
struct ConvexState {
    thread: String,
    panel: String,
    configured: Deployment,
    choices: Vec<Choice>,
    note: Option<String>,
    loading_choices: bool,
    /// The key for the deployment shown, once the runner's made it.
    key: Option<Key>,
    /// The deployment a key is being asked for.
    fetching: Option<String>,
    /// Why there's no key: the runner's words and its error code.
    failure: Option<(String, i64)>,
    sign_in: Option<Login>,
    sign_in_failure: Option<String>,
    sign_in_task: Option<Task<()>>,
    /// The deployment menu's up (native, and modal while it is).
    menu_open: bool,
}

pub(crate) struct WebPanels {
    pages: Vec<Kept>,
    /// Why a page couldn't be made, by its key.
    failed: HashMap<String, String>,
    /// Pages asked for this frame, made just after it.
    making: HashSet<String>,
    convex: HashMap<String, ConvexState>,
    /// The deployment each Convex panel shows, by the one it's configured
    /// with, so threads in the same project share it (the Swift app's
    /// `convexPanelDeployments`).
    selections: HashMap<String, String>,
    /// Deployments whose New deploy key was pressed, until it's asked for.
    renewing: HashSet<String>,
    client: Option<Arc<Client>>,
    /// Links that left a panel for the browser, newest last (the dump's).
    left: Vec<String>,
    /// Whether something could be over the pages in the last frame.
    covered: bool,
}

struct Shared(Entity<WebPanels>);
impl Global for Shared {}

pub(crate) fn init(cx: &mut App) {
    let selections = load_selections();
    let entity = cx.new(|_| WebPanels { pages: Vec::new(), failed: HashMap::new(), making: HashSet::new(), convex: HashMap::new(), selections, renewing: HashSet::new(), client: None, left: Vec::new(), covered: false });
    cx.set_global(Shared(entity.clone()));
    // The app draws the panels, so it's told when a page, a Convex panel or
    // the runtime changes.
    // ⌘C and the like while a page has the keyboard are the page's.
    cx.intercept_keystrokes(|event, _, cx| {
        if edit_key(&event.keystroke, cx) {
            cx.stop_propagation();
        }
    })
    .detach();
    cx.observe_new(move |app: &mut Threadbear, _: Option<&mut Window>, cx: &mut Context<Threadbear>| {
        let client = app.client.clone();
        entity.update(cx, |p, _| p.client = Some(client));
        cx.observe(&entity, |_, _, cx| cx.notify()).detach();
        if let Some(runtime) = web_runtime::runtime(cx) {
            cx.observe(&runtime, |_, _, cx| cx.notify()).detach();
        }
    })
    .detach();
}

fn panels(cx: &App) -> Option<Entity<WebPanels>> {
    cx.try_global::<Shared>().map(|s| s.0.clone())
}

thread_local! {
    /// Each page's count of clicks and keys when last asked, and when it
    /// last changed, by its key (`holds_a_restart`).
    static USED: std::cell::RefCell<HashMap<String, (u64, Option<std::time::Instant>)>> = std::cell::RefCell::new(HashMap::new());
}

/// Whether a web panel's page holds an update's restart off: what's typed
/// in a page can't be read from here, so one with the keyboard, or clicked
/// or typed in within the last minute, counts as being typed in (core's
/// `web_page_holds`). Asked once a second (app_update.rs).
pub(crate) fn holds_a_restart(cx: &App) -> bool {
    let Some(panels) = panels(cx) else { return false };
    let now = std::time::Instant::now();
    USED.with(|used| {
        let mut used = used.borrow_mut();
        let pages = &panels.read(cx).pages;
        used.retain(|key, _| pages.iter().any(|k| &k.key == key));
        let mut holds = false;
        for kept in pages {
            let count = kept.page.input_count();
            // A page first seen here hasn't been used since: only a change
            // in its count from here on counts.
            let seen = used.entry(kept.key.clone()).or_insert((count, None));
            if seen.0 != count {
                *seen = (count, Some(now));
            }
            let ago = seen.1.map(|at| now.duration_since(at).as_secs_f64());
            holds |= threadbear_core::app_update::web_page_holds(kept.page.has_keyboard(), ago);
        }
        holds
    })
}

/// ⌘C, ⌘X, ⌘V, ⌘A, ⌘Z and ⇧⌘Z while a page has the keyboard: that page's
/// own Edit command, as the Swift app's Edit menu sends copy: to its web
/// view. GPUI's view sees key equivalents before the page does (it answers
/// performKeyEquivalent for the whole window), so without this they'd go to
/// whatever GPUI has focused, the composer say. True when the page took it.
fn edit_key(keystroke: &Keystroke, cx: &App) -> bool {
    let m = &keystroke.modifiers;
    if !m.platform || m.control || m.alt || m.function {
        return false;
    }
    let command = match (keystroke.key.as_str(), m.shift) {
        ("c", false) => "copy",
        ("x", false) => "cut",
        ("v", false) => "paste",
        ("a", false) => "selectAll",
        ("z", false) => "undo",
        ("z", true) => "redo",
        _ => return false,
    };
    let Some(panels) = panels(cx) else { return false };
    let p = panels.read(cx);
    let Some(kept) = p.pages.iter().find(|k| !k.page.hidden() && k.page.has_keyboard()) else { return false };
    kept.page.edit(command);
    EDITS.with(|e| {
        let mut e = e.borrow_mut();
        e.push(command.to_owned());
        let extra = e.len().saturating_sub(10);
        e.drain(..extra);
    });
    true
}

thread_local! {
    /// The Edit commands a page took from the keyboard lately, for the dump.
    static EDITS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------- the frame

/// Made as the main window's frame starts (Threadbear's render) and drawn
/// after everything else in it: counts the frame, then once the panels are
/// laid out shows the pages placed in it, where they were placed. `menu_at`
/// is where one of the app's menus is open, if one is; `popover` is the share
/// popover being up.
pub(crate) fn sweeper(menu_at: Option<Point<Pixels>>, popover: bool) -> AnyElement {
    let frame = FRAME.with(|f| {
        f.set(f.get() + 1);
        f.get()
    });
    canvas(
        move |_, window, cx| sweep(frame, menu_at, popover, window, cx),
        |_, _, window, _| {
            // A press anywhere GPUI draws is one outside every page (a page's
            // own view takes the presses over it), so the keyboard comes back
            // from a page that had it, as a click in another view takes it in
            // any Mac app. GPUI's view never asks for it on a click itself.
            window.on_mouse_event(|_: &MouseDownEvent, phase, window, _| {
                if phase.capture() && threadbear_web::keyboard_holder(native_view(window).unwrap_or(std::ptr::null_mut())) == threadbear_web::KeyboardHolder::Page {
                    keyboard_to_app(window);
                }
            });
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}

/// Gives GPUI's view its window's keyboard. Called wherever the app asks GPUI
/// to focus something, at that moment: AppKit's keyboard follows GPUI's focus
/// as the Swift app's makeFirstResponder does, even when it's the same box
/// again (a picture dropped on the composer while a page had the keyboard),
/// and a page clicked after the request keeps it. GPUI's view never takes it
/// back by itself. `focus_requests_take_the_keyboard` checks every request
/// does this.
pub(crate) fn keyboard_to_app(window: &Window) {
    if let Some(view) = native_view(window) {
        threadbear_web::take_keyboard(view);
    }
}

/// The automation's `native-keys`: `text` typed through AppKit, so the keys
/// go wherever AppKit sends a person's (`keyboard` in the dump).
pub(crate) fn send_keys(window: AnyWindowHandle, text: &str, cx: &mut AsyncApp) -> Result<(), String> {
    let view = cx.update_window(window, |_, window, _| native_view(window).map(|v| v as usize)).map_err(|e| e.to_string())?.ok_or("the window has no view")?;
    threadbear_web::send_keys(view as *mut c_void, text);
    Ok(())
}

/// Who has the main window's keyboard, for the dump: "app" (GPUI's own
/// view), "page", "hidden page", "window" (nobody) or "other".
pub(crate) fn keyboard_dump(window: &Window) -> Value {
    json!(threadbear_web::keyboard_holder(native_view(window).unwrap_or(std::ptr::null_mut())).name())
}

fn sweep(frame: u64, menu_at: Option<Point<Pixels>>, popover: bool, window: &mut Window, cx: &mut App) {
    let Some(panels) = panels(cx) else { return };
    let modal = window.has_active_dialog(cx) || window.has_active_sheet(cx) || popover;
    let height = f32::from(window.viewport_size().height);
    let mut covered_any = false;
    for kept in &panels.read(cx).pages {
        let (stamp, b) = kept.drawn.get();
        // A menu opens down and to the right of where it was asked for, about
        // this wide: one reaching the page's column covers it.
        let menu_over = menu_at.is_some_and(|at| at.x + px(300.) > b.origin.x && at.x < b.origin.x + b.size.width);
        let covered = modal || menu_over;
        covered_any |= covered && stamp == frame;
        if stamp == frame && !covered && f32::from(b.size.width) >= 1. && f32::from(b.size.height) >= 1. {
            kept.page.set_bounds(f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height), height);
            kept.page.set_hidden(false);
        } else {
            kept.page.set_hidden(true);
        }
    }
    if panels.read(cx).covered != covered_any {
        panels.update(cx, |p, _| p.covered = covered_any);
    }
    // A page's view that had the keyboard and went out of sight (⌘N's
    // new-thread page, another thread, a menu over it) leaves it with the
    // window itself, which beeps at every letter while GPUI draws its caret
    // and ⌘V still reaches GPUI (Mike, 12 Nov, a dictated paste and then
    // nothing he typed; features/tools/real-typing.ts plays it with real keys).
    use threadbear_web::KeyboardHolder;
    let holder = threadbear_web::keyboard_holder(native_view(window).unwrap_or(std::ptr::null_mut()));
    if matches!(holder, KeyboardHolder::HiddenPage | KeyboardHolder::Window | KeyboardHolder::Other) {
        keyboard_to_app(window);
    }
}

/// The window's own view, a page's parent.
fn native_view(window: &Window) -> Option<*mut c_void> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

// ---------------------------------------------------------------- the pool

impl WebPanels {
    /// The page for `key`, moved to the end as the one used last.
    fn touch(&mut self, key: &str) -> Option<Rc<Cell<(u64, Bounds<Pixels>)>>> {
        let at = self.pages.iter().position(|k| k.key == key)?;
        let kept = self.pages.remove(at);
        let drawn = kept.drawn.clone();
        self.pages.push(kept);
        Some(drawn)
    }

    fn page(&self, key: &str) -> Option<&Rc<PanelPage>> {
        self.pages.iter().find(|k| k.key == key).map(|k| &k.page)
    }

    /// Makes the page for `key` in `parent`, closing the one used longest ago
    /// if that makes more than four.
    fn make(&mut self, key: String, url: String, kind: PanelKind, parent: *mut c_void, cx: &mut Context<Self>) {
        self.making.remove(&key);
        if self.page(&key).is_some() {
            return;
        }
        match PanelPage::open(parent, &url, kind) {
            Ok((page, mut events)) => {
                let named = key.clone();
                let task = cx.spawn(async move |this, cx| {
                    while let Some(event) = events.next().await {
                        if this.update(cx, |p, cx| p.page_said(&named, event, cx)).is_err() {
                            break;
                        }
                    }
                });
                self.failed.remove(&key);
                self.pages.push(Kept { key, page: Rc::new(page), drawn: Rc::new(Cell::new((0, Bounds::default()))), _events: task });
                while self.pages.len() > CAPACITY {
                    // Never the one just made: it's last.
                    self.pages.remove(0).page.set_hidden(true);
                }
            }
            Err(why) => {
                self.failed.insert(key, why);
            }
        }
        cx.notify();
    }

    fn page_said(&mut self, key: &str, event: PanelEvent, cx: &mut Context<Self>) {
        match event {
            PanelEvent::Leave(url) => {
                // As a link from a highlight goes: the browser, for now (the
                // Swift app can also open it over the panel, with Back).
                self.left.push(url.clone());
                if self.left.len() > 20 {
                    self.left.remove(0);
                }
                crate::open_gate::open_web(&url, cx);
            }
            PanelEvent::Changed => {}
        }
        let _ = key;
        cx.notify();
    }

    /// Drops a page (New deploy key): the next look makes a fresh one.
    fn forget(&mut self, key: &str) {
        // Out of sight now: the frame that drew it may hold on to it a
        // moment longer, and the sweep no longer sees it.
        for kept in self.pages.iter().filter(|k| k.key == key) {
            kept.page.set_hidden(true);
        }
        self.pages.retain(|k| k.key != key);
        self.failed.remove(key);
    }
}

/// show_panel's `reload` (`reloadRequested`): a url panel's page back to
/// its own address, so what the agent just changed is what Mike sees (the
/// app closes a preview opened over it). A page not made yet loads fresh
/// when it's drawn, so there's nothing to do for it.
pub(crate) fn reload_requested(thread: &ThreadSummary, panel_id: &str, cx: &mut App) {
    let Some(panel) = thread.panels().into_iter().find(|p| p.id == panel_id && p.kind == "url") else { return };
    let Some(panels) = panels(cx) else { return };
    let key = match &panel.convex {
        Some(configured) => format!("convex:{}", panels.read(cx).selected(configured)),
        None => match panel.url.clone().filter(|u| !u.trim().is_empty()) {
            Some(url) => url,
            None => return,
        },
    };
    if let Some(page) = panels.read(cx).page(&key).cloned() {
        page.reload();
    }
}

/// The pane's slot for a page: a page kept from before, placed here this
/// frame, or (the first time) one made just after it. `parent` is the
/// window's view.
fn slot(panels: &Entity<WebPanels>, key: &str, url: &str, kind: impl FnOnce() -> PanelKind, parent: *mut c_void, open: Option<Opener>, cx: &mut App) -> AnyElement {
    if let Some(drawn) = panels.update(cx, |p, _| p.touch(key)) {
        return canvas(move |bounds, _, _| drawn.set((frame_now(), bounds)), |_, _, _, _| {}).size_full().into_any_element();
    }
    if let Some(why) = panels.read(cx).failed.get(key).cloned() {
        return away(&format!("Couldn't show this page: {why}"), open, cx);
    }
    if panels.update(cx, |p, _| p.making.insert(key.to_owned())) {
        let (panels, key, url, kind) = (panels.clone(), key.to_owned(), url.to_owned(), kind());
        // After this frame: Chromium makes its view as it's asked, which
        // shouldn't happen in the middle of the app's drawing.
        cx.defer(move |cx| panels.update(cx, |p, cx| p.make(key, url, kind, parent, cx)));
    }
    div().size_full().into_any_element()
}

// ---------------------------------------------------------------- drawing

/// What Open in browser opens, and its button's tip.
#[derive(Clone)]
struct Opener {
    url: String,
    tip: String,
}

/// A panel's section in the pane, drawn here rather than by the side pane so
/// its page is placed every frame (the side pane is a cached view, redrawn
/// only when it changes). None for a panel that isn't a page.
pub(crate) fn section(app: &Threadbear, thread: &ThreadSummary, panel_id: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let panel = thread.panels().into_iter().find(|p| p.id == panel_id && p.kind == "url")?;
    let panels = panels(cx)?;
    let main = window.window_handle() == app.main_window;
    let body = match &panel.convex {
        Some(configured) => convex_view(&panels, thread, &panel, configured, main, window, cx),
        None => page_view(&panels, &panel, main, window, cx),
    };
    Some(div().id(SharedString::from(format!("web-panel-{panel_id}"))).size_full().child(body).into_any_element())
}

/// What a patch's url panel can be: a web page, or one of the app's own
/// data: pages (the mock runner's stand-ins). A file or any other scheme
/// isn't loaded in a panel, which keeps its sign-ins: it opens elsewhere.
fn loads_in_panel(url: &str) -> bool {
    threadbear_web::site(url).is_some() || url.starts_with("data:")
}

fn page_view(panels: &Entity<WebPanels>, panel: &Panel, main: bool, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(url) = panel.url.clone().filter(|u| !u.trim().is_empty()) else {
        return note("This panel has no page to show.", cx);
    };
    let open = Some(Opener { url: url.clone(), tip: "Open in browser".into() });
    if !loads_in_panel(&url) {
        return away("This panel's page isn't a web page, so it opens outside the app.", open, cx);
    }
    match running(&format!("web-panel-{}", panel.id), "this panel's page", main, open.clone(), window, cx) {
        Ok(parent) => slot(panels, &url, &url, || PanelKind::Patch, parent, open, cx),
        Err(instead) => instead,
    }
}

/// The window's view once Chromium's running here, or what to show until it
/// is: the question, why not, or a note while it starts.
fn running(id: &str, wants: &str, main: bool, open: Option<Opener>, window: &mut Window, cx: &mut App) -> Result<*mut c_void, AnyElement> {
    if !main {
        // Its view is one per page, kept in the main window.
        return Err(away("This panel's page shows when it's in the main window.", open, cx));
    }
    let Some(runtime) = web_runtime::runtime(cx) else { return Err(away("There's no web runtime here, so this page opens in your browser.", open, cx)) };
    if runtime.read(cx).state == State::Ready {
        if runtime.update(cx, |r, cx| r.ensure_engine(cx)) {
            return native_view(window).ok_or_else(|| away("The window has no view to show the page in.", open, cx));
        }
        return Err(match &runtime.read(cx).engine {
            Engine::Failed(why) => away(&format!("The web runtime didn't start: {why}"), open, cx),
            _ => starting(cx),
        });
    }
    if let Some(prompt) = web_runtime::prompt(id, wants, cx) {
        let p = palette(cx);
        return Err(v_flex().size_full().items_center().justify_center().gap(px(14.)).p(px(24.)).child(div().max_w(px(320.)).child(prompt)).children(open.map(|o| open_button(&o, p, cx))).into_any_element());
    }
    let why = match &runtime.read(cx).state {
        State::Unavailable => "There's no web runtime for this system yet, so this page opens in your browser.",
        _ => "This page shows here once the web runtime is installed (Settings > General).",
    };
    Err(away(why, open, cx))
}

fn starting(cx: &App) -> AnyElement {
    let secondary = style::secondary_text(cx);
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .child(Spinner::new().color(secondary).with_size(px(14.)))
        .child(div().text_size(px(style::size::LABEL)).text_color(secondary).child("Starting the web runtime…"))
        .into_any_element()
}

/// A grey line in the middle of the pane (`PaneNote`).
fn note(text: &str, cx: &App) -> AnyElement {
    crate::files::pane_note(text, palette(cx)).into_any_element()
}

/// A line saying why the page isn't here, and Open in browser.
fn away(text: &str, open: Option<Opener>, cx: &App) -> AnyElement {
    let p = palette(cx);
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .p(px(24.))
        .child(div().max_w(px(320.)).text_center().text_size(px(style::size::LABEL)).line_height(px(18.)).text_color(p.secondary).child(text.to_owned()))
        .children(open.map(|o| open_button(&o, p, cx)))
        .into_any_element()
}

fn open_button(open: &Opener, _p: Palette, cx: &App) -> AnyElement {
    let url = open.url.clone();
    PushButton::new("web-panel-open", "Open in browser", open.tip.clone()).render(cx, move |_, _, cx| crate::files::open_web(&url, cx))
}

// ---------------------------------------------------------------- the bar

/// A pane panel's buttons at the end of its bar in the dock (`DockPaneButtons`):
/// for a page, Back once it's moved on from its own page, Reload and Open in
/// browser; for the Convex panel, New deploy key, Reload and Open in browser.
pub(crate) fn bar_buttons(app: &Threadbear, panel_id: &str, cx: &mut App) -> Option<AnyElement> {
    let thread = app.pane_thread()?;
    let panel = thread.panels().into_iter().find(|p| p.id == panel_id && p.kind == "url")?;
    // Over a preview the side pane draws its own bar.
    if app.side.read(cx).previews.get(&thread.id).is_some_and(|pv| pv.from == panel_id) {
        return None;
    }
    let panels = panels(cx)?;
    let p = palette(cx);
    let mut row = h_flex().flex_none().items_center().pr(px(4.));
    if let Some(configured) = &panel.convex {
        let selected = panels.read(cx).selected(configured);
        let key = format!("convex:{selected}");
        let page = panels.read(cx).page(&key).cloned();
        // A key and a reload only mean something with the dashboard here.
        let here = web_runtime::runtime(cx).is_some_and(|r| r.read(cx).engine == Engine::Running);
        if here {
            let (renew, reload) = (panels.clone(), page.clone());
            let deployment = selected.clone();
            row = row.child(bar_button("web-bar-key", "key", format!("Make a new deploy key for {selected}, if the old one was revoked"), false, p, move |cx| {
                renew.update(cx, |p, cx| p.renew(&deployment, cx));
            }));
            row = row.child(bar_button("web-bar-reload", "arrow.clockwise", "Reload panel".into(), page.as_ref().is_some_and(|pg| pg.loading()), p, move |_| {
                if let Some(page) = &reload {
                    page.reload();
                }
            }));
        }
        if let Some(url) = convex::browser_url(&selected) {
            row = row.child(bar_button("web-bar-open", "arrow.up.right", format!("Open {selected} in browser"), false, p, move |cx| {
                crate::files::open_web(&url, cx);
            }));
        }
        return Some(row.into_any_element());
    }
    let url = panel.url.clone().filter(|u| !u.trim().is_empty())?;
    let page = panels.read(cx).page(&url).cloned();
    // Its own page moved on to another (a same-site link, a script's
    // window.open): back a page at a time.
    if let Some(page) = page.clone().filter(|pg| pg.can_go_back()) {
        row = row.child(bar_button("web-bar-back", "chevron.left", "Back".into(), false, p, move |_| page.go_back()));
    }
    if let Some(page) = page.clone() {
        let loading = page.loading();
        row = row.child(bar_button("web-bar-reload", "arrow.clockwise", "Reload the panel's own page".into(), loading, p, move |_| page.reload()));
    }
    row = row.child(bar_button("web-bar-open", "arrow.up.right", "Open in browser".into(), false, p, move |cx| {
        // The panel's page is the patch's to say: a web page opens, anything
        // else goes through the gate's rules as any link does.
        if loads_in_panel(&url) && !url.starts_with("data:") {
            crate::files::open_web(&url, cx);
        } else {
            crate::open_gate::open_link(&url, crate::open_gate::Origin::OtherMac, "", cx);
        }
    }));
    Some(row.into_any_element())
}

/// `PaneHeaderButton`: a 26-point square with its symbol, grey until the
/// pointer's on it. `busy` shows a spinner in its place while what it
/// started is under way (a reload), and still takes a click.
fn bar_button(id: &'static str, symbol: &str, tip: String, busy: bool, p: Palette, action: impl Fn(&mut App) + 'static) -> AnyElement {
    let hover = p.hover;
    let fg = p.fg;
    let body: AnyElement = if busy { Spinner::new().color(p.secondary).with_size(px(11.)).into_any_element() } else { icon(symbol, style::size::LABEL, p.secondary).into_any_element() };
    tipped(div().id(id).size(px(26.)).flex().flex_none().items_center().justify_center().rounded(px(6.)).hover(move |s| s.bg(hover).text_color(fg)).cursor_pointer().child(body), tip)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| action(cx))
        .into_any_element()
}

// ---------------------------------------------------------------- Convex

impl WebPanels {
    fn selected(&self, configured: &Deployment) -> String {
        self.selections.get(&configured.deployment).cloned().unwrap_or_else(|| configured.deployment.clone())
    }

    fn select(&mut self, configured: &Deployment, deployment: &str, cx: &mut Context<Self>) {
        self.selections.insert(configured.deployment.clone(), deployment.to_owned());
        save_selections(&self.selections);
        // Every panel showing that project now wants that deployment's key.
        let states: Vec<String> = self.convex.iter().filter(|(_, s)| s.configured.deployment == configured.deployment).map(|(k, _)| k.clone()).collect();
        for key in states {
            self.fetch(&key, cx);
        }
        cx.notify();
    }

    /// The panel's state, made (and its lookups started) the first time it's drawn.
    fn ensure_convex(&mut self, state_key: &str, thread: &ThreadSummary, panel: &Panel, configured: &Deployment, keys: bool, cx: &mut Context<Self>) {
        if !self.convex.contains_key(state_key) {
            self.convex.insert(state_key.to_owned(), ConvexState { thread: thread.id.clone(), panel: panel.id.clone(), configured: configured.clone(), loading_choices: true, ..Default::default() });
            self.load_choices(state_key, false, cx);
        }
        // A key only once there's a page to give it to: the runner makes
        // one on Convex the first time.
        let state = &self.convex[state_key];
        let wants = self.selected(configured);
        let has = state.key.as_ref().is_some_and(|k| k.deployment == wants) || state.fetching.as_deref() == Some(wants.as_str()) || state.failure.is_some() || state.sign_in.is_some();
        if keys && !has {
            self.fetch(state_key, cx);
        }
    }

    fn load_choices(&mut self, state_key: &str, refresh: bool, cx: &mut Context<Self>) {
        let (Some(client), Some(state)) = (self.client.clone(), self.convex.get_mut(state_key)) else { return };
        state.loading_choices = true;
        let params = json!({ "threadId": state.thread, "panelId": state.panel, "refresh": refresh });
        let state_key = state_key.to_owned();
        cx.spawn(async move |this, cx| {
            let answer = crate::call(client, "panels.convexDeployments", params).await;
            this.update(cx, |p, cx| {
                let selected = p.convex.get(&state_key).map(|s| p.selected(&s.configured));
                let Some(state) = p.convex.get_mut(&state_key) else { return };
                match answer.map(|v| serde_json::from_value::<convex::Choices>(v).unwrap_or_default()) {
                    Ok(listed) => {
                        state.choices = convex::with_configured(listed.deployments, &state.configured);
                        state.note = listed.note;
                    }
                    Err(e) => {
                        state.choices = convex::with_configured(Vec::new(), &state.configured);
                        state.note = Some(format!("Couldn't load other deployments: {}", e.message));
                    }
                }
                state.loading_choices = false;
                let configured = state.configured.clone();
                // A remembered choice the project no longer has: back to its own.
                if selected.is_some_and(|s| !state.choices.iter().any(|c| c.deployment == s)) {
                    p.select(&configured, &configured.deployment, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Asks the runner for the shown deployment's key (`panels.convexKey`),
    /// renewing it if New deploy key was pressed for it.
    fn fetch(&mut self, state_key: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(configured) = self.convex.get(state_key).map(|s| s.configured.clone()) else { return };
        let target = self.selected(&configured);
        let renew = self.renewing.remove(&target);
        let Some(state) = self.convex.get_mut(state_key) else { return };
        if renew || state.key.as_ref().is_some_and(|k| k.deployment != target) {
            state.key = None;
        }
        state.failure = None;
        state.fetching = Some(target.clone());
        let params = json!({ "threadId": state.thread, "panelId": state.panel, "deployment": target, "renew": renew });
        let state_key = state_key.to_owned();
        cx.spawn(async move |this, cx| {
            let answer = crate::call(client, "panels.convexKey", params).await;
            this.update(cx, |p, cx| {
                let now = p.convex.get(&state_key).map(|s| p.selected(&s.configured));
                let Some(state) = p.convex.get_mut(&state_key) else { return };
                // A newer choice was made while this was on its way.
                if now.as_deref() != Some(target.as_str()) || state.fetching.as_deref() != Some(target.as_str()) {
                    return;
                }
                state.fetching = None;
                match answer.map(serde_json::from_value::<Key>) {
                    Ok(Ok(key)) => {
                        state.key = Some(key);
                        state.sign_in_failure = None;
                    }
                    Ok(Err(_)) => state.failure = Some(("The runner's answer had no deploy key in it.".into(), -32000)),
                    Err(e) => state.failure = Some((e.message, e.code)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// New deploy key: drops the page with the old key and asks for a fresh one.
    fn renew(&mut self, deployment: &str, cx: &mut Context<Self>) {
        self.forget(&format!("convex:{deployment}"));
        self.renewing.insert(deployment.to_owned());
        let states: Vec<String> = self.convex.iter().filter(|(_, s)| self.selected(&s.configured) == deployment).map(|(k, _)| k.clone()).collect();
        for key in states {
            if let Some(state) = self.convex.get_mut(&key) {
                state.key = None;
            }
            self.fetch(&key, cx);
        }
        cx.notify();
    }

    /// Signs the runner's Convex CLI in (`panels.convexLogin`): opens its page
    /// in the browser on this Mac (any browser can approve it, so another
    /// Mac's runner works too), polls until it's done, then asks for the key.
    fn sign_in(&mut self, state_key: &str, force: bool, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(state) = self.convex.get_mut(state_key) else { return };
        state.sign_in_failure = None;
        state.sign_in = Some(Login { state: "waiting".into(), ..Default::default() });
        let (thread, panel) = (state.thread.clone(), state.panel.clone());
        let key = state_key.to_owned();
        let task = cx.spawn(async move |this, cx| {
            let mut answer = crate::call(client.clone(), "panels.convexLogin", json!({ "threadId": thread, "panelId": panel, "force": force })).await.map(|v| serde_json::from_value::<Login>(v).unwrap_or_default());
            if let Ok(started) = &answer {
                if let Some(page) = started.page() {
                    let _ = cx.update(|cx| crate::files::open_web(&page, cx));
                }
            }
            loop {
                let now = match &answer {
                    Ok(now) if now.state == "waiting" => now.clone(),
                    _ => break,
                };
                // A page that isn't Convex's, first or on a later look:
                // nothing to open, and the sign-in stops saying so.
                if let Some(refused) = now.refused_page() {
                    let _ = this.update(cx, |p, cx| {
                        if let Some(s) = p.convex.get_mut(&key) {
                            s.sign_in = None;
                            s.sign_in_failure = Some(refused);
                        }
                        cx.notify();
                    });
                    return;
                }
                let _ = this.update(cx, |p, cx| {
                    if let Some(s) = p.convex.get_mut(&key) {
                        s.sign_in = Some(now);
                    }
                    cx.notify();
                });
                cx.background_executor().timer(Duration::from_secs(2)).await;
                answer = crate::call(client.clone(), "panels.convexLogin", json!({ "threadId": thread, "panelId": panel, "check": true })).await.map(|v| serde_json::from_value::<Login>(v).unwrap_or_default());
            }
            let _ = this.update(cx, |p, cx| {
                let Some(s) = p.convex.get_mut(&key) else { return };
                s.sign_in = None;
                match answer {
                    Ok(done) if done.state == "done" => {
                        s.failure = None;
                        p.load_choices(&key, true, cx);
                        p.fetch(&key, cx);
                    }
                    Ok(other) => s.sign_in_failure = Some(other.message.unwrap_or_else(|| "The sign-in didn't finish. Try again.".into())),
                    Err(e) => s.sign_in_failure = Some(e.message),
                }
                cx.notify();
            });
        });
        if let Some(state) = self.convex.get_mut(state_key) {
            state.sign_in_task = Some(task);
        }
        cx.notify();
    }

    fn stop_sign_in(&mut self, state_key: &str, cx: &mut Context<Self>) {
        if let Some(state) = self.convex.get_mut(state_key) {
            state.sign_in_task = None;
            state.sign_in = None;
        }
        cx.notify();
    }
}

/// Where Convex's embedded dashboard is: Convex's own, or in a scripted app
/// a stand-in a spec serves (THREADBEAR_CONVEX_EMBEDDED), so the key's way
/// into the page can be checked without Convex. Never in a release build:
/// the page gets a deploy key.
fn embedded() -> String {
    #[cfg(feature = "automation")]
    if let Some(url) = std::env::var("THREADBEAR_CONVEX_EMBEDDED").ok().filter(|u| u.starts_with("http://127.0.0.1:")) {
        return url;
    }
    convex::EMBEDDED.to_owned()
}

fn convex_view(panels: &Entity<WebPanels>, thread: &ThreadSummary, panel: &Panel, configured: &Deployment, main: bool, window: &mut Window, cx: &mut App) -> AnyElement {
    let state_key = format!("{}/{}", thread.id, panel.id);
    let selected = panels.read(cx).selected(configured);
    let open = convex::browser_url(&selected).map(|url| Opener { url, tip: format!("Open {selected} in browser") });
    // The page, if Chromium's running here; else what's shown instead.
    let parent = running(&format!("convex-panel-{}", panel.id), "the Convex dashboard here", main, open.clone(), window, cx);
    panels.update(cx, |p, cx| p.ensure_convex(&state_key, thread, panel, configured, parent.is_ok(), cx));
    let header = convex_header(panels, &state_key, configured, &selected, window, cx);
    let body = match parent {
        Err(instead) => instead,
        Ok(parent) => convex_body(panels, &state_key, &selected, parent, open, cx),
    };
    v_flex().size_full().child(header).child(div().h(px(1.)).flex_none().bg(style::hairline(cx))).child(div().flex_1().min_h_0().child(body)).into_any_element()
}

/// The project, the deployment menu and the deployment's name, with the
/// runner's note on what it couldn't list under them.
fn convex_header(panels: &Entity<WebPanels>, state_key: &str, configured: &Deployment, selected: &str, window: &mut Window, cx: &mut App) -> AnyElement {
    let p = palette(cx);
    let state = panels.read(cx).convex.get(state_key);
    let loading = state.is_none_or(|s| s.loading_choices);
    let choices = state.map(|s| s.choices.clone()).unwrap_or_default();
    let label = convex::label(&choices, selected).to_owned();
    let note = state.and_then(|s| s.note.clone());
    let mut row = h_flex().gap(px(8.)).items_center().min_w_0();
    if let (Some(project), Some(tip)) = (&configured.project, configured.project_tip()) {
        row = row.child(tipped(div().id("convex-project").flex_none().text_size(px(style::size::LABEL)).font_weight(FontWeight::SEMIBOLD).text_color(p.fg).child(project.clone()), tip));
    }
    let view = native_view(window);
    let (menu_panels, menu_key, menu_configured) = (panels.clone(), state_key.to_owned(), configured.clone());
    let hover = p.hover;
    let mut menu = div()
        .id("convex-deployment-menu")
        .flex_none()
        .h(px(22.))
        .px(px(6.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(5.))
        .text_size(px(style::size::LABEL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.fg)
        .child(label)
        .child(icon("chevron.down", style::size::SMALL - 2., p.secondary));
    if loading {
        menu = menu.opacity(0.5);
    } else {
        menu = menu.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |event, _, cx| {
            let at = event.position();
            open_deployment_menu(&menu_panels, &menu_key, &menu_configured, view, at, cx);
        });
    }
    row = row.child(tipped(menu, "Choose the deployment shown in this panel"));
    row = row.child(tipped(div().id("convex-deployment").min_w_0().truncate().font_family(cx.theme().mono_font_family.clone()).text_size(px(style::size::SMALL)).text_color(p.secondary).child(selected.to_owned()), selected.to_owned()));
    row = row.child(div().flex_1());
    if loading {
        row = row.child(Spinner::new().color(p.secondary).with_size(px(10.)));
    }
    let mut header = v_flex().flex_none().gap(px(6.)).px(px(12.)).py(px(8.)).child(row);
    if let Some(note) = note {
        header = header.child(tipped(div().id("convex-choices-note").text_size(px(style::size::SMALL)).line_height(px(15.)).text_color(p.secondary).line_clamp(2).child(note.clone()), note));
    }
    header.into_any_element()
}

/// The deployment menu, native so it comes up over the page: each choice
/// with its label and name, ticked when shown, then Refresh deployments.
fn open_deployment_menu(panels: &Entity<WebPanels>, state_key: &str, configured: &Deployment, view: Option<*mut c_void>, at: Point<Pixels>, cx: &mut App) {
    let Some(view) = view else { return };
    let (choices, selected) = {
        let p = panels.read(cx);
        (p.convex.get(state_key).map(|s| s.choices.clone()).unwrap_or_default(), p.selected(configured))
    };
    let mut items: Vec<threadbear_web::MenuItem> = choices
        .iter()
        .map(|c| threadbear_web::MenuItem { title: format!("{} · {}", c.label, c.deployment), symbol: "server.rack".into(), tip: format!("Show {} in this panel", c.deployment), checked: c.deployment == selected })
        .collect();
    items.push(threadbear_web::MenuItem::default());
    items.push(threadbear_web::MenuItem { title: "Refresh deployments".into(), symbol: "arrow.clockwise".into(), tip: "Look up this project's development and production deployments again".into(), checked: false });
    let (panels, state_key, configured) = (panels.clone(), state_key.to_owned(), configured.clone());
    panels.update(cx, |p, cx| {
        if let Some(s) = p.convex.get_mut(&state_key) {
            s.menu_open = true;
        }
        cx.notify();
    });
    // From a task, outside the app's update: the menu's loop runs until it's
    // closed, and the windows go on drawing behind it (as alert.rs's do).
    cx.spawn(async move |cx| {
        let chosen = threadbear_web::popup_menu(view, f32::from(at.x), f32::from(at.y) + 4., &items);
        cx.update(|cx| {
            panels.update(cx, |p, cx| {
                if let Some(s) = p.convex.get_mut(&state_key) {
                    s.menu_open = false;
                }
                match chosen {
                    Some(i) if i < choices.len() => p.select(&configured, &choices[i].deployment, cx),
                    Some(i) if i == items.len() - 1 => p.load_choices(&state_key, true, cx),
                    _ => {}
                }
                cx.notify();
            })
        });
    })
    .detach();
}

/// The dashboard once there's a key, the sign-in under way, why there's no
/// key with what to do about it, or a note while it's asked for.
fn convex_body(panels: &Entity<WebPanels>, state_key: &str, selected: &str, parent: *mut c_void, open: Option<Opener>, cx: &mut App) -> AnyElement {
    let p = palette(cx);
    let (key, sign_in, failure, sign_in_failure, loading) = {
        let s = panels.read(cx).convex.get(state_key);
        (s.and_then(|s| s.key.clone()), s.and_then(|s| s.sign_in.clone()), s.and_then(|s| s.failure.clone()), s.and_then(|s| s.sign_in_failure.clone()), s.is_none_or(|s| s.loading_choices))
    };
    if let Some(key) = key.filter(|k| k.deployment == selected) {
        let url = embedded();
        let origin = convex::origin_of(&url);
        let script = move || PanelKind::Convex { credentials_script: convex::credentials_script(&key, &origin) };
        return slot(panels, &format!("convex:{selected}"), &url, script, parent, open, cx);
    }
    let words = |text: String, color: Hsla, weight: FontWeight| div().max_w(px(320.)).text_center().text_size(px(style::size::LABEL)).line_height(px(18.)).font_weight(weight).text_color(color).child(text);
    let column = || v_flex().size_full().items_center().justify_center().gap(px(12.)).p(px(24.));
    if let Some(login) = sign_in.filter(|l| l.state == "waiting") {
        let stop = (panels.clone(), state_key.to_owned());
        let mut buttons = h_flex().gap(px(8.));
        if let Some(page) = login.page() {
            buttons = buttons.child(PushButton::new("convex-sign-in-again", "Open the page again", "Open Convex's sign-in page in your browser").render(cx, move |_, _, cx| crate::files::open_web(&page, cx)));
        }
        buttons = buttons.child(PushButton::new("convex-sign-in-cancel", "Cancel", "Stop waiting for the sign-in").render(cx, move |_, _, cx| stop.0.update(cx, |p, cx| p.stop_sign_in(&stop.1, cx))));
        return column()
            .child(Spinner::new().color(p.secondary).with_size(px(14.)))
            .child(words("Approve the sign-in in your browser.".into(), p.fg, FontWeight::MEDIUM))
            .children(login.code.map(|code| words(format!("Convex shows the code {code}. The dashboard opens here once you've approved it."), p.secondary, FontWeight::NORMAL)))
            .child(buttons)
            .into_any_element();
    }
    if let Some((message, code)) = failure {
        let mut col = column();
        if code == convex::SIGNED_OUT {
            col = col.child(words("Sign in to Convex to see this dashboard.".into(), p.fg, FontWeight::MEDIUM)).child(words("Your browser opens to approve it, then the dashboard loads here by itself.".into(), p.secondary, FontWeight::NORMAL));
        } else {
            col = col.child(words(message, p.secondary, FontWeight::NORMAL));
        }
        if let Some(why) = sign_in_failure {
            col = col.child(div().max_w(px(320.)).text_center().text_size(px(style::size::SMALL)).text_color(p.secondary).child(why));
        }
        let mut buttons = h_flex().gap(px(8.));
        if code == convex::SIGNED_OUT {
            let go = (panels.clone(), state_key.to_owned());
            buttons = buttons.child(PushButton::new("convex-sign-in", "Sign in to Convex", "Sign the runner's Convex CLI in, in your browser").render(cx, move |_, _, cx| go.0.update(cx, |p, cx| p.sign_in(&go.1, false, cx))));
        } else if code == convex::NO_ACCESS {
            let go = (panels.clone(), state_key.to_owned());
            buttons = buttons.child(PushButton::new("convex-sign-in-other", "Sign in as someone else", "Sign the runner's Convex CLI in with another Convex account").render(cx, move |_, _, cx| go.0.update(cx, |p, cx| p.sign_in(&go.1, true, cx))));
        }
        let again = (panels.clone(), state_key.to_owned());
        buttons = buttons.child(PushButton::new("convex-try-again", "Try again", "Ask the runner for a deploy key again").render(cx, move |_, _, cx| again.0.update(cx, |p, cx| p.fetch(&again.1, cx))));
        return col.child(buttons).into_any_element();
    }
    note(&if loading { "Finding deployments…".to_owned() } else { format!("Getting a deploy key for {selected}…") }, cx)
}

// ---------------------------------------------------------------- remembered

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    /// The deployment shown, by the one a panel is configured with.
    convex_deployments: HashMap<String, String>,
}

/// A small file in the data folder, as the Files panel's choices are
/// (files_store.rs); the Swift app keeps these in UserDefaults.
fn store_path() -> std::path::PathBuf {
    threadbear_client::config::RunnerConfig::from_env().home.join("desktop-web-panels.json")
}

fn load_selections() -> HashMap<String, String> {
    std::fs::read_to_string(store_path()).ok().and_then(|text| serde_json::from_str::<Stored>(&text).ok()).unwrap_or_default().convex_deployments
}

fn save_selections(selections: &HashMap<String, String>) {
    if let Ok(text) = serde_json::to_string_pretty(&Stored { convex_deployments: selections.clone() }) {
        // Best effort: it's a preference, not data. Written whole, and only
        // this user's, as everything in the data folder is.
        let _ = threadbear_core::private_file::write(&store_path(), text.as_bytes());
    }
}

// ---------------------------------------------------------------- dumps

fn page_dump(key: &str, kept: Option<&Kept>, frame: u64) -> Value {
    match kept {
        None => json!({ "key": key }),
        Some(k) => {
            let (stamp, b) = k.drawn.get();
            let (x, y, w, h) = k.page.bounds();
            json!({
                "key": key,
                "url": k.key.strip_prefix("convex:").map_or(k.key.clone(), |_| embedded()),
                "address": k.page.address(),
                "title": k.page.title(),
                "loading": k.page.loading(),
                "loads": k.page.loads(),
                "canGoBack": k.page.can_go_back(),
                "hidden": k.page.hidden(),
                "muted": k.page.muted(),
                "hasKeyboard": k.page.has_keyboard(),
                "placedThisFrame": stamp == frame,
                "slot": [f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)],
                "frame": [x, y, w, h],
            })
        }
    }
}

/// The dump's `webPanels`: every page kept, and the links that left one.
pub(crate) fn dump(cx: &App) -> Value {
    let Some(panels) = panels(cx) else { return Value::Null };
    let p = panels.read(cx);
    let frame = frame_now();
    json!({
        "pages": p.pages.iter().map(|k| page_dump(&k.key, Some(k), frame)).collect::<Vec<_>>(),
        "left": p.left,
        "covered": p.covered,
        "edits": EDITS.with(|e| e.borrow().clone()),
    })
}

/// The pane's `web`, for a page panel that's showing: what it shows (a page,
/// the runtime's question, why it can't, or a note), its bar's buttons, and
/// for the Convex panel its header and state.
pub(crate) fn fill_pane_dump(pane: &mut serde_json::Map<String, Value>, thread: &ThreadSummary, id: &str, cx: &App) {
    let Some(panel) = thread.panels().into_iter().find(|p| p.id == id && p.kind == "url") else { return };
    let Some(panels) = panels(cx) else { return };
    let p = panels.read(cx);
    let runtime = web_runtime::runtime(cx).map(|r| r.read(cx));
    let ready = runtime.is_some_and(|r| r.state == State::Ready && r.engine == Engine::Running);
    let blocked = match runtime {
        _ if ready => None,
        Some(r) if r.asks() => Some(("prompt", String::new())),
        Some(r) if r.state == State::Ready => Some(("starting", match &r.engine {
            Engine::Failed(why) => format!("The web runtime didn't start: {why}"),
            _ => "Starting the web runtime…".into(),
        })),
        Some(r) if r.state == State::Unavailable => Some(("note", "There's no web runtime for this system yet, so this page opens in your browser.".into())),
        _ => Some(("note", "This page shows here once the web runtime is installed (Settings > General).".into())),
    };
    let mut web = serde_json::Map::new();
    let page_key;
    let mut buttons: Vec<String> = Vec::new();
    if let Some(configured) = &panel.convex {
        let selected = p.selected(configured);
        page_key = format!("convex:{selected}");
        let state = p.convex.get(&format!("{}/{}", thread.id, panel.id));
        let choices = state.map(|s| s.choices.clone()).unwrap_or_default();
        let shows = match (blocked.as_ref(), state) {
            (Some((what, _)), _) => (*what).to_owned(),
            (None, None) => "finding".into(),
            (None, Some(s)) if s.key.as_ref().is_some_and(|k| k.deployment == selected) => if p.page(&page_key).is_some() { "page".into() } else { "loading".into() },
            (None, Some(s)) if s.sign_in.as_ref().is_some_and(|l| l.state == "waiting") => "signingIn".into(),
            (None, Some(s)) if s.failure.is_some() => "failed".into(),
            (None, Some(s)) if s.loading_choices => "finding".into(),
            _ => "gettingKey".into(),
        };
        web.insert("shows".into(), json!(shows));
        web.insert(
            "convex".into(),
            json!({
                "project": configured.project,
                "label": convex::label(&choices, &selected),
                "deployment": selected,
                "choices": choices.iter().map(|c| format!("{} · {}", c.label, c.deployment)).collect::<Vec<_>>(),
                "note": state.and_then(|s| s.note.clone()),
                "failure": state.and_then(|s| s.failure.as_ref().map(|f| f.0.clone())),
                "code": state.and_then(|s| s.sign_in.as_ref().and_then(|l| l.code.clone())),
                "signInFailure": state.and_then(|s| s.sign_in_failure.clone()),
                "menuOpen": state.is_some_and(|s| s.menu_open),
            }),
        );
        if ready {
            buttons.push(format!("Make a new deploy key for {selected}, if the old one was revoked"));
            buttons.push("Reload panel".into());
        }
        if convex::browser_url(&selected).is_some() {
            buttons.push(format!("Open {selected} in browser"));
        }
    } else {
        let url = panel.url.clone().unwrap_or_default();
        page_key = url.clone();
        web.insert("url".into(), json!(url));
        let shows = match blocked.as_ref() {
            _ if url.trim().is_empty() || !loads_in_panel(&url) => "note".to_owned(),
            Some((what, _)) => (*what).to_owned(),
            None if p.failed.contains_key(&url) => "failed".into(),
            None if p.page(&url).is_some() => "page".into(),
            None => "loading".into(),
        };
        web.insert("shows".into(), json!(shows));
        if let Some(page) = p.page(&url) {
            if page.can_go_back() {
                buttons.push("Back".into());
            }
            buttons.push("Reload the panel's own page".into());
        }
        buttons.push("Open in browser".into());
    }
    if let Some((_, why)) = blocked.filter(|(_, why)| !why.is_empty()) {
        web.insert("note".into(), json!(why));
    }
    if let Some(why) = p.failed.get(&page_key) {
        web.insert("note".into(), json!(format!("Couldn't show this page: {why}")));
    }
    web.insert("buttons".into(), json!(buttons));
    let kept = p.pages.iter().find(|k| k.key == page_key);
    if kept.is_some() {
        web.insert("page".into(), page_dump(&page_key, kept, frame_now()));
    }
    pane.insert("web".into(), Value::Object(web));
}

// ---------------------------------------------------------------- automation

/// `{"cmd":"webPanel","do":"eval"|"click"|"drop"|"focus"|"wait"|"choose", "key"?, "js"?, "selector"?, "times"?, "file"?, "title"?, "deployment"?, "wait"?}`:
/// a script's way into a panel's page, the one shown last unless `key` names
/// another. `eval` runs `js` in the page and answers its value; `click`
/// clicks the element `selector` finds with a person's gesture behind it
/// (DevTools' `userGesture`, which is what decides whether a link leaves the
/// panel; `times` clicks again straight after); `drop` drops the file at
/// `file` on it as the Finder
/// would; `focus` gives the page
/// the keyboard, as a click in it does; `wait` waits until it's done loading
/// (and has `title`, if given). Each answers the page's dump. `choose` picks
/// `deployment` in the Convex panel that offers it.
#[cfg(feature = "automation")]
pub(crate) async fn automation(cmd: &Value, cx: &mut AsyncApp) -> Result<Value, String> {
    let wait = Duration::from_secs_f64(cmd["wait"].as_f64().unwrap_or(10.));
    let since = std::time::Instant::now();
    // `choose`: a pick from a Convex panel's deployment menu, which is a
    // native menu a script can't reach.
    if cmd["do"] == "choose" {
        let deployment = cmd["deployment"].as_str().ok_or("choose needs deployment")?.to_owned();
        return cx.update(|cx| {
            let panels = panels(cx).ok_or("no web panels")?;
            panels.update(cx, |p, cx| {
                let configured = p.convex.values().find(|s| s.choices.iter().any(|c| c.deployment == deployment)).map(|s| s.configured.clone()).ok_or(format!("no Convex panel offers {deployment}"))?;
                p.select(&configured, &deployment, cx);
                Ok(json!({ "chosen": deployment }))
            })
        });
    }
    let find = |cx: &mut AsyncApp| -> Option<(String, Rc<PanelPage>)> {
        cx.update(|cx| {
            let panels = panels(cx)?;
            let p = panels.read(cx);
            let kept = match cmd["key"].as_str() {
                Some(key) => p.pages.iter().find(|k| k.key == key),
                None => p.pages.iter().max_by_key(|k| k.drawn.get().0),
            }?;
            Some((kept.key.clone(), kept.page.clone()))
        })
    };
    // The page may still be on its way (made just after the frame that asked for it).
    let (key, page) = loop {
        if let Some(found) = find(cx) {
            break found;
        }
        if since.elapsed() > wait {
            return Err("there's no panel page".into());
        }
        cx.background_executor().timer(Duration::from_millis(100)).await;
    };
    match cmd["do"].as_str().unwrap_or("") {
        "eval" => {
            let js = cmd["js"].as_str().ok_or("eval needs js")?;
            let answer = page.devtools("Runtime.evaluate", json!({ "expression": js, "returnByValue": true, "awaitPromise": true })).await.map_err(|_| "the page went away".to_owned())??;
            if let Some(why) = answer.get("exceptionDetails") {
                return Err(format!("the script threw: {}", why["exception"]["description"].as_str().or(why["text"].as_str()).unwrap_or("?")));
            }
            return Ok(json!({ "value": answer["result"]["value"], "page": cx.update(|cx| dump_page(&key, cx)) }));
        }
        "click" => {
            let selector = cmd["selector"].as_str().ok_or("click needs selector")?;
            // The element's own click, run with a person's gesture behind it
            // (DevTools' `userGesture`), which is what the panel's rules go
            // by. Mouse events posted through DevTools' Input domain went
            // missing now and then on a busy check host (no new window was
            // even asked for), and a gesture is all they'd add.
            let click = format!(
                "(() => {{ const e = document.querySelector({}); if (!e) return false; e.scrollIntoView({{ block: 'center' }}); e.click(); return true; }})()",
                serde_json::to_string(selector).unwrap_or_default()
            );
            // `times`: clicks one straight after another, each a gesture of
            // its own, as fast as DevTools takes them.
            for _ in 0..cmd["times"].as_u64().unwrap_or(1).clamp(1, 10) {
                // Counted as a person's click in the page, as AppKit counts
                // a real one (LeaveGate).
                page.note_input();
                let done = page.devtools("Runtime.evaluate", json!({ "expression": click, "returnByValue": true, "userGesture": true })).await.map_err(|_| "the page went away".to_owned())?;
                match done {
                    Ok(done) if done["result"]["value"] != json!(true) => return Err(format!("nothing in the page matches {selector}")),
                    Ok(_) => {}
                    // The click took the page somewhere before DevTools could
                    // answer: it worked.
                    Err(why) if why.contains("navigated or closed") => {}
                    Err(why) => return Err(why),
                }
            }
        }
        // A file from the Finder dropped on the page at (`x`, `y`), through
        // DevTools' Input domain as a real drop is delivered: `file` is its path.
        "drop" => {
            let file = cmd["file"].as_str().ok_or("drop needs file")?;
            let (x, y) = (cmd["x"].as_f64().unwrap_or(60.), cmd["y"].as_f64().unwrap_or(60.));
            // Copy, link or move, as the Finder offers.
            let data = json!({ "items": [], "files": [file], "dragOperationsMask": 1 | 2 | 16 });
            for kind in ["dragEnter", "dragOver", "drop"] {
                page.devtools("Input.dispatchDragEvent", json!({ "type": kind, "x": x, "y": y, "data": data })).await.map_err(|_| "the page went away".to_owned())??;
            }
        }
        // The keyboard, as a click in the page gives it. A scripted app's
        // window is never in front, so Chromium would take the page as
        // unfocused, as a page in a window behind others is, and leave
        // Select All and the like undone: it's told to act as a page in the
        // front window does.
        "focus" => {
            page.focus();
            page.devtools("Emulation.setFocusEmulationEnabled", json!({ "enabled": true })).await.map_err(|_| "the page went away".to_owned())??;
        }
        "wait" => {
            let title = cmd["title"].as_str();
            loop {
                let done = !page.loading() && title.is_none_or(|t| page.title() == t);
                if done {
                    break;
                }
                if since.elapsed() > wait {
                    return Err(format!("the page didn't finish (title {:?}, loading {})", page.title(), page.loading()));
                }
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
        }
        other => return Err(format!("do must be eval, click, drop, focus, wait or choose, not {other:?}")),
    }
    Ok(cx.update(|cx| dump_page(&key, cx)))
}

#[cfg(feature = "automation")]
fn dump_page(key: &str, cx: &App) -> Value {
    let Some(panels) = panels(cx) else { return Value::Null };
    let p = panels.read(cx);
    page_dump(key, p.pages.iter().find(|k| k.key == key), frame_now())
}
