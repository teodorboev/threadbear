// A web panel's page (panel.web, panel.convex): a site a patch or the Convex
// panel names, in a native view placed from the pane's layout slot each
// frame, as page.rs places a sealed one. Unlike a sealed page it runs its
// scripts and keeps its sign-ins (the panels' own request context in the
// profile, as the Swift app's WebPanelPool uses WebKit's default store),
// except the Convex dashboard's, which gets a context that keeps nothing as
// its deploy key is in it. As WebPanelPool:
//
// - A link, a form or a new window to another site, clicked, leaves the page
//   where it is and goes to the app (Event::Leave), which opens it the way a
//   link from a highlight goes (Mike, 11 Nov: a click on a video loaded
//   YouTube in the panel with no way back). A redirect or a script's own
//   navigation stays in a patch's panel, so a sign-in that goes through
//   another site and back still works; in the Convex dashboard it's
//   cancelled. Nothing goes out without a person's click, one page for each
//   click and at most one a second (panel_rules: Codex review, 11 Nov, a
//   script could otherwise open a browser tab a second).
// - A same-site new window, clicked, loads in the same view: there's no
//   other window to give it. The Convex dashboard's links out all leave.
// - Only web addresses load as the panel's page: a file dropped on the page,
//   or a file or data: page in a new window, doesn't. Its iframes are
//   Chromium's to rule on (it keeps files out of them).
// - Nothing downloads; Chromium's View Source isn't offered; what a page
//   logs to its console stays out of Chromium's log.
// - The app hears when it can go back, and when it's loading.
use super::host::{tb_view_has_keyboard, tb_view_input_count, tb_view_note_input, tb_view_remove, tb_view_set_frame, tb_view_set_hidden, tb_view_set_parent, tb_view_watch_input};
use crate::panel_rules::{browse, popup, site, Browse, Event, Kind, LeaveGate, Popup};
use cef::*;
use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value as Json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// What the page's handlers share with it. They run on Chromium's UI
/// thread, which is the app's main thread here, but CEF wants them Send.
struct Shared {
    /// Scheme, host and port of the panel's own page.
    home: Mutex<String>,
    /// The address the app loaded in the panel: the only page that isn't a
    /// web address (a patch's data: page) that may load in it.
    own: Mutex<String>,
    /// How often a page may go from this panel to the browser.
    gate: Mutex<LeaveGate>,
    /// The page's own view (its address), whose clicks and keys AppKit
    /// counts for the gate.
    view: AtomicUsize,
    convex: bool,
    can_go_back: AtomicBool,
    loading: AtomicBool,
    /// Main-frame loads started, so a dump can tell a page kept between
    /// visits from one loaded again.
    loads: AtomicU32,
    title: Mutex<String>,
    address: Mutex<String>,
    events: Mutex<Option<mpsc::UnboundedSender<Event>>>,
    calls: Mutex<HashMap<i32, oneshot::Sender<Result<Json, String>>>>,
    /// A page to load once DevTools has answered this message: the Convex
    /// dashboard, once its credentials script is sure to run in it.
    load_after: Mutex<Option<(i32, String)>>,
    gone: AtomicBool,
}

impl Shared {
    fn send(&self, event: Event) {
        if let Some(tx) = self.events.lock().ok().and_then(|e| e.clone()) {
            let _ = tx.unbounded_send(event);
        }
    }

    fn home(&self) -> String {
        self.home.lock().map(|h| h.clone()).unwrap_or_default()
    }

    fn own(&self) -> String {
        self.own.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// A page for the browser, one for each click and at most one a second
    /// from this panel; any more are dropped without a word (`LeaveGate`).
    fn leave(&self, url: String) {
        let inputs = unsafe { tb_view_input_count(self.view.load(Ordering::Acquire) as *mut c_void) };
        let allowed = self.gate.lock().is_ok_and(|mut gate| gate.allow(std::time::Instant::now(), inputs));
        if allowed {
            self.send(Event::Leave(url));
        }
    }

    fn fail_calls(&self, why: &str) {
        self.gone.store(true, Ordering::Release);
        let calls: Vec<_> = self.calls.lock().map(|mut c| c.drain().map(|(_, tx)| tx).collect()).unwrap_or_default();
        for tx in calls {
            let _ = tx.send(Err(why.to_owned()));
        }
    }
}

thread_local! {
    /// The Convex dashboards' context: kept in memory, gone with the app.
    static CONVEX_CONTEXT: RefCell<Option<RequestContext>> = const { RefCell::new(None) };
}

fn convex_context() -> Option<RequestContext> {
    CONVEX_CONTEXT.with(|c| {
        if c.borrow().is_none() {
            // No cache path: in memory only, shared with nothing else.
            *c.borrow_mut() = request_context_create_context(Some(&RequestContextSettings::default()), None);
        }
        c.borrow().clone()
    })
}

pub struct PanelPage {
    browser: Browser,
    view: *mut c_void,
    parent: Cell<*mut c_void>,
    /// Where it was last put: x, y, width, height and the parent's height,
    /// which the flip to AppKit's coordinates depends on.
    frame: Cell<(f32, f32, f32, f32, f32)>,
    hidden: Cell<bool>,
    /// The panel's own page, which Reload goes back to.
    url: RefCell<String>,
    shared: Arc<Shared>,
    next: AtomicI32,
    /// Its DevTools answers' observer, let go of before the page closes.
    observer: Option<Registration>,
}

impl PanelPage {
    /// Opens `url` in a child view of `parent` (a window's own NSView),
    /// hidden until it's first placed. The receiver hears what the page tells
    /// the app.
    pub fn open(parent: *mut c_void, url: &str, kind: Kind) -> Result<(PanelPage, mpsc::UnboundedReceiver<Event>), String> {
        if super::host::status() != super::host::Status::Ready {
            return Err("the web runtime isn't running".into());
        }
        if parent.is_null() {
            return Err("the window has no view to put the page in".into());
        }
        let (tx, rx) = mpsc::unbounded();
        let shared = Arc::new(Shared {
            home: Mutex::new(site(url).unwrap_or_default()),
            own: Mutex::new(url.to_owned()),
            gate: Mutex::new(LeaveGate::default()),
            view: AtomicUsize::new(0),
            convex: matches!(kind, Kind::Convex { .. }),
            can_go_back: AtomicBool::new(false),
            loading: AtomicBool::new(true),
            loads: AtomicU32::new(0),
            title: Mutex::new(String::new()),
            address: Mutex::new(String::new()),
            events: Mutex::new(Some(tx)),
            calls: Mutex::new(HashMap::new()),
            load_after: Mutex::new(None),
            gone: AtomicBool::new(false),
        });
        // Alloy (no Chrome toolbar or menus), a native child view.
        let window_info = WindowInfo { runtime_style: RuntimeStyle::ALLOY, ..WindowInfo::default() }.set_as_child(parent, &Rect { x: 0, y: 0, width: 1, height: 1 });
        // A page reads the clipboard only when Mike pastes, as in a browser.
        let settings = BrowserSettings { javascript_access_clipboard: State::DISABLED, ..Default::default() };
        let mut client = PanelClient::new(
            PanelRequests::new(shared.clone()),
            PanelLife::new(shared.clone()),
            PanelLoads::new(shared.clone()),
            PanelDisplay::new(shared.clone()),
            PanelMenu::new(),
            super::sealed::NoDownloads::new(),
        );
        // The Convex dashboard starts blank, so its credentials answer is in
        // place before its page's first script runs.
        let first = match &kind {
            Kind::Patch => url.to_owned(),
            Kind::Convex { .. } => "about:blank".to_owned(),
        };
        let mut context = match &kind {
            Kind::Patch => request_context_get_global_context(),
            Kind::Convex { .. } => convex_context(),
        };
        let browser = browser_host_create_browser_sync(Some(&window_info), Some(&mut client), Some(&CefString::from(first.as_str())), Some(&settings), None, context.as_mut())
            .ok_or("Chromium didn't make the page")?;
        let host = browser.host().ok_or("the page has no host")?;
        let view = host.window_handle();
        // A person's clicks and keys in it, counted for the gate.
        shared.view.store(view as usize, Ordering::Release);
        unsafe { tb_view_watch_input(view) };
        let mut observer = Answers::new(shared.clone());
        let registration = host.add_dev_tools_message_observer(Some(&mut observer));
        // Out of sight until the app puts it somewhere.
        unsafe { tb_view_set_hidden(view, true) };
        let page = PanelPage {
            browser,
            view,
            parent: Cell::new(parent),
            frame: Cell::new((0., 0., 0., 0., 0.)),
            hidden: Cell::new(true),
            url: RefCell::new(url.to_owned()),
            shared,
            next: AtomicI32::new(1),
            observer: registration,
        };
        if let Kind::Convex { credentials_script } = kind {
            // In every frame from the start, as a WKUserScript at document
            // start; the script itself only answers on the dashboard's origin
            // and only the page's own request.
            // The dashboard loads once DevTools has the script (its answer
            // comes back on Chromium's loop, in Answers): loaded straight
            // away, the page's first scripts ran before it and its request
            // went unanswered.
            drop(page.devtools("Page.enable", json!({})));
            let id = page.next.load(Ordering::Relaxed);
            if let Ok(mut after) = page.shared.load_after.lock() {
                *after = Some((id, url.to_owned()));
            }
            drop(page.devtools("Page.addScriptToEvaluateOnNewDocument", json!({ "source": credentials_script })));
        }
        Ok((page, rx))
    }

    /// A DevTools method on the page (chromedevtools.github.io/devtools-protocol),
    /// its result when it comes. In-process, as the offscreen pages ask theirs.
    pub fn devtools(&self, method: &str, params: Json) -> oneshot::Receiver<Result<Json, String>> {
        let (tx, rx) = oneshot::channel();
        if self.shared.gone.load(Ordering::Acquire) {
            let _ = tx.send(Err("its page closed".into()));
            return rx;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut calls) = self.shared.calls.lock() {
            calls.insert(id, tx);
        }
        let message = json!({ "id": id, "method": method, "params": params }).to_string();
        let sent = self.browser.host().map(|h| h.send_dev_tools_message(Some(message.as_bytes()))).unwrap_or(0);
        if sent != 1 {
            if let Some(tx) = self.shared.calls.lock().ok().and_then(|mut c| c.remove(&id)) {
                let _ = tx.send(Err(format!("DevTools didn't take {method}")));
            }
        }
        rx
    }

    /// Loads `url` as the panel's own page.
    pub fn load(&self, url: &str) {
        if let Ok(mut home) = self.shared.home.lock() {
            *home = site(url).unwrap_or_default();
        }
        if let Ok(mut own) = self.shared.own.lock() {
            *own = url.to_owned();
        }
        *self.url.borrow_mut() = url.to_owned();
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }

    /// Reload: back to the panel's own page, as the Swift bar's Reload does,
    /// wherever its links have taken it since.
    pub fn reload(&self) {
        let url = self.url.borrow().clone();
        self.load(&url);
    }

    pub fn can_go_back(&self) -> bool {
        self.shared.can_go_back.load(Ordering::Acquire)
    }

    pub fn loading(&self) -> bool {
        self.shared.loading.load(Ordering::Acquire)
    }

    pub fn loads(&self) -> u32 {
        self.shared.loads.load(Ordering::Acquire)
    }

    pub fn title(&self) -> String {
        self.shared.title.lock().map(|t| t.clone()).unwrap_or_default()
    }

    /// Where the page is now: its own page, or wherever a same-site link took it.
    pub fn address(&self) -> String {
        self.shared.address.lock().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn go_back(&self) {
        self.browser.go_back();
    }

    /// Into another window's view: the panel was moved to a window of its
    /// own, or back. AppKit moves the view; Chromium follows its window.
    pub fn move_to(&self, parent: *mut c_void) {
        if parent.is_null() || parent == self.parent.get() {
            return;
        }
        self.parent.set(parent);
        unsafe { tb_view_set_parent(self.view, parent) };
        self.frame.set((0., 0., 0., 0., 0.));
    }

    pub fn parent(&self) -> *mut c_void {
        self.parent.get()
    }

    /// Places it in a GPUI layout slot: window points, from the top left.
    /// `parent_height` is the window's content height, as the flip into
    /// AppKit's coordinates needs it.
    pub fn set_bounds(&self, x: f32, y: f32, width: f32, height: f32, parent_height: f32) {
        let frame = (x, y, width, height, parent_height);
        if self.frame.get() == frame {
            return;
        }
        self.frame.set(frame);
        unsafe { tb_view_set_frame(self.view, f64::from(x), f64::from(y), f64::from(width), f64::from(height)) };
        if let Some(host) = self.browser.host() {
            host.was_resized();
        }
    }

    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let (x, y, w, h, _) = self.frame.get();
        (x, y, w, h)
    }

    /// Out of sight: hidden, its videos and sounds paused, and muted until
    /// it's back, so the page's own script can't start them again unheard.
    /// The pool keeps a page loaded when its panel is closed or another
    /// thread is picked, and a video in the Swift app's played on with
    /// nothing on screen to stop it (Mike, 11 Nov; FirstClickWebView pauses
    /// its page the same way). Coming back unmutes, and leaves them paused.
    pub fn set_hidden(&self, hidden: bool) {
        if self.hidden.get() == hidden {
            return;
        }
        self.hidden.set(hidden);
        unsafe { tb_view_set_hidden(self.view, hidden) };
        if hidden {
            self.pause_media();
        }
        if let Some(host) = self.browser.host() {
            host.was_hidden(i32::from(hidden));
            host.set_audio_muted(i32::from(hidden));
        }
    }

    /// Pauses every video and sound in the page, in each of its frames.
    fn pause_media(&self) {
        const PAUSE: &str = "for (const m of document.querySelectorAll('video, audio')) { try { m.pause(); } catch (e) {} }";
        let mut ids = CefStringList::new();
        self.browser.frame_identifiers(Some(&mut ids));
        for id in ids {
            if let Some(frame) = self.browser.frame_by_identifier(Some(&CefString::from(id.as_str()))) {
                frame.execute_java_script(Some(&CefString::from(PAUSE)), None, 0);
            }
        }
    }

    pub fn hidden(&self) -> bool {
        self.hidden.get()
    }

    pub fn muted(&self) -> bool {
        self.browser.host().is_some_and(|h| h.is_audio_muted() == 1)
    }

    /// One of the page's own Edit commands, for ⌘C and the like while it has
    /// the keyboard: the app's key bindings would otherwise take them.
    pub fn edit(&self, command: &str) {
        let Some(frame) = self.browser.focused_frame().or_else(|| self.browser.main_frame()) else { return };
        match command {
            "copy" => frame.copy(),
            "cut" => frame.cut(),
            "paste" => frame.paste(),
            "selectAll" => frame.select_all(),
            "undo" => frame.undo(),
            "redo" => frame.redo(),
            _ => {}
        }
    }

    /// Whether the page's view, or one inside it, has the keyboard.
    pub fn has_keyboard(&self) -> bool {
        unsafe { tb_view_has_keyboard(self.view) }
    }

    /// How many clicks and keys a person has given the page's view so far:
    /// one changing says it's being used (an update's restart waits for it).
    pub fn input_count(&self) -> u64 {
        unsafe { tb_view_input_count(self.view) }
    }

    /// A scripted app's stand-in for a person's click in the page: the
    /// specs click through DevTools, which AppKit never sees (`LeaveGate`).
    pub fn note_input(&self) {
        unsafe { tb_view_note_input(self.view) };
    }

    /// Gives the page the keyboard, as a click in it does.
    pub fn focus(&self) {
        if let Some(host) = self.browser.host() {
            host.set_focus(1);
        }
    }
}

impl Drop for PanelPage {
    fn drop(&mut self) {
        if let Ok(mut events) = self.shared.events.lock() {
            events.take();
        }
        self.shared.fail_calls("its page closed");
        // Its observer goes while the page it watches is still there.
        self.observer = None;
        // A child page ends when its view goes. CloseBrowser would ask the
        // window it's in to close (CEF's CloseHostWindow sends the window
        // performClose:), and New deploy key took the app's main window
        // with it (11 Nov). The view's parent holds the only reference to
        // it, so taking it out ends the browser (CefBrowserHostView's
        // dealloc tells CEF its window is gone, and CEF destroys it).
        unsafe { tb_view_remove(self.view) };
    }
}

wrap_client! {
    struct PanelClient {
        requests: RequestHandler,
        life: LifeSpanHandler,
        loads: LoadHandler,
        display: DisplayHandler,
        menu: ContextMenuHandler,
        downloads: DownloadHandler,
    }

    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.requests.clone())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.loads.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.menu.clone())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(self.downloads.clone())
        }
    }
}

wrap_request_handler! {
    struct PanelRequests {
        shared: Arc<Shared>,
    }

    impl RequestHandler {
        // The panel's own frame: panel_rules::browse says what happens (an
        // iframe's are Chromium's to rule on).
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, user_gesture: c_int, is_redirect: c_int) -> c_int {
            if !frame.is_some_and(|f| f.is_main() == 1) {
                return 0;
            }
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            match browse(&url, &self.shared.home(), &self.shared.own(), self.shared.convex, user_gesture == 1, is_redirect == 1) {
                Browse::Stay => 0,
                Browse::Leave => {
                    self.shared.leave(url);
                    1
                }
                Browse::Drop => 1,
            }
        }
    }
}

wrap_life_span_handler! {
    struct PanelLife {
        shared: Arc<Shared>,
    }

    impl LifeSpanHandler {
        // No new windows: a click's to another site leaves, one on the same
        // site loads in this view (WebPanelPool's createWebViewWith), and a
        // script's own is dropped (panel_rules::popup).
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            match popup(&url, &self.shared.home(), self.shared.convex, user_gesture == 1) {
                Popup::Leave => self.shared.leave(url),
                Popup::LoadHere => {
                    if let Some(frame) = browser.and_then(|b| b.main_frame()) {
                        frame.load_url(Some(&CefString::from(url.as_str())));
                    }
                }
                Popup::Drop => {}
            }
            1
        }

        // Never the window the page is in: a close asked for any other way
        // (a page's own window.close()) would otherwise close the app's main
        // window. The page's own end is its view going (PanelPage's drop),
        // which CEF doesn't ask about.
        fn do_close(&self, _browser: Option<&mut Browser>) -> c_int {
            1
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            self.shared.fail_calls("its page closed");
        }
    }
}

wrap_load_handler! {
    struct PanelLoads {
        shared: Arc<Shared>,
    }

    impl LoadHandler {
        fn on_loading_state_change(&self, _browser: Option<&mut Browser>, is_loading: c_int, can_go_back: c_int, _can_go_forward: c_int) {
            self.shared.loading.store(is_loading == 1, Ordering::Release);
            self.shared.can_go_back.store(can_go_back == 1, Ordering::Release);
            self.shared.send(Event::Changed);
        }

        fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition_type: TransitionType) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                self.shared.loads.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
}

wrap_display_handler! {
    struct PanelDisplay {
        shared: Arc<Shared>,
    }

    impl DisplayHandler {
        // What a page says to its console stays out of Chromium's log
        // (web-runtime.log), as the sealed pages' does: a dashboard's code,
        // or a script on its site, could log the deploy key (Codex review,
        // 11 Nov).
        fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity, _message: Option<&CefString>, _source: Option<&CefString>, _line: c_int) -> c_int {
            1
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Ok(mut now) = self.shared.title.lock() {
                *now = title.map(|t| t.to_string()).unwrap_or_default();
            }
            self.shared.send(Event::Changed);
        }

        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if !frame.is_some_and(|f| f.is_main() == 1) {
                return;
            }
            if let Ok(mut now) = self.shared.address.lock() {
                *now = url.map(|u| u.to_string()).unwrap_or_default();
            }
            self.shared.send(Event::Changed);
        }
    }
}

wrap_context_menu_handler! {
    struct PanelMenu;

    impl ContextMenuHandler {
        // Chromium's own menu, less View Source (which would open a window
        // of its own) and Print.
        fn on_before_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _params: Option<&mut ContextMenuParams>, model: Option<&mut MenuModel>) {
            if let Some(model) = model {
                // MENU_ID_VIEW_SOURCE and MENU_ID_PRINT (cef_menu_id_t).
                model.remove(132);
                model.remove(131);
            }
        }
    }
}

wrap_dev_tools_message_observer! {
    struct Answers {
        shared: Arc<Shared>,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, browser: Option<&mut Browser>, message_id: c_int, success: c_int, result: Option<&[u8]>) {
            // Loaded from a task of its own on Chromium's loop, not from in
            // here: a navigation started inside DevTools' own callback sends
            // it events while it's still telling its observers this answer,
            // which Chromium flags ("Check failed: !check_reentrancy"), and
            // the app went down soon after (11 Nov).
            let then = self.shared.load_after.lock().ok().and_then(|mut after| after.take_if(|(id, _)| *id == message_id));
            if let (Some((_, url)), Some(browser)) = (then, browser) {
                let mut task = LoadLater::new(browser.clone(), url);
                post_task(ThreadId::UI, Some(&mut task));
            }
            let Some(tx) = self.shared.calls.lock().ok().and_then(|mut c| c.remove(&message_id)) else { return };
            let parsed = result.and_then(|bytes| serde_json::from_slice::<Json>(bytes).ok()).unwrap_or(Json::Null);
            let answer = if success == 1 { Ok(parsed) } else { Err(parsed["message"].as_str().unwrap_or("DevTools said no").to_owned()) };
            let _ = tx.send(answer);
        }
    }
}

// The Convex dashboard's first load, once its credentials script is in.
wrap_task! {
    struct LoadLater {
        browser: Browser,
        url: String,
    }

    impl Task {
        fn execute(&self) {
            if let Some(frame) = self.browser.main_frame() {
                frame.load_url(Some(&CefString::from(self.url.as_str())));
            }
        }
    }
}
