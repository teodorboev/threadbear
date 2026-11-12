// CEF inside the GPUI app on a Mac, started the way Ghostex's Mac path and the
// gpui-web-spike branch start it (MIT; the notice is in native/appkit_hooks.m):
// the framework loaded from the data folder, GPUI's NSApplication given
// CefAppProtocol, and from a run-loop timer cef::initialize and then
// cef::run_message_loop, which nests Chromium's loop inside GPUI's for the
// rest of the process. Started the first time something needs it, not at
// launch, so a Mac that never shows a diagram never runs Chromium.
//
// Everything here runs on the main thread, which is also Chromium's UI
// thread: its callbacks arrive inside its loop, outside any GPUI update, so
// they only ever touch their own state and send on channels.
use super::sealed;
use cef::{args::Args, *};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::Mutex;

unsafe extern "C" {
    fn tb_cef_install_app_hooks();
    fn tb_cef_start_message_loop();
    // Placing a page's own view in a GPUI layout slot (page.rs).
    pub(crate) fn tb_view_set_frame(view: *mut c_void, x: f64, y: f64, width: f64, height: f64);
    pub(crate) fn tb_view_set_hidden(view: *mut c_void, hidden: bool);
    // A panel's page moving between windows, and whether it has the keyboard (panel.rs).
    pub(crate) fn tb_view_set_parent(view: *mut c_void, parent: *mut c_void);
    pub(crate) fn tb_view_remove(view: *mut c_void);
    // A person's clicks and keys in a page's view, counted (panel.rs).
    pub(crate) fn tb_view_watch_input(view: *mut c_void);
    pub(crate) fn tb_view_input_count(view: *mut c_void) -> u64;
    pub(crate) fn tb_view_note_input(view: *mut c_void);
    pub(crate) fn tb_view_has_keyboard(view: *mut c_void) -> bool;
    // Who has a window's keyboard, and giving it back to GPUI's view.
    fn tb_view_keyboard_holder(view: *mut c_void) -> i32;
    fn tb_view_take_keyboard(view: *mut c_void) -> bool;
    fn tb_view_send_keys(view: *mut c_void, text: *const c_char);
    fn tb_popup_menu(view: *mut c_void, x: f64, y: f64, titles: *const *const c_char, symbols: *const *const c_char, tips: *const *const c_char, checked: *const bool, count: i32) -> i32;
    fn tb_main_bundle_identifier() -> *const c_char;
}

/// One item of a native menu: an empty title is a separator.
#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    pub title: String,
    /// An SF Symbol's name, beside the title.
    pub symbol: String,
    pub tip: String,
    pub checked: bool,
}

/// A native menu at (x, y) in a window's view (GPUI's coordinates), for a
/// menu over a web page's view, which GPUI's own would draw under. Blocks
/// while it's open, so call it from a task, outside any update (as the app's
/// alerts are); returns the index of the item chosen.
pub fn popup_menu(view: *mut c_void, x: f32, y: f32, items: &[MenuItem]) -> Option<usize> {
    if view.is_null() || items.is_empty() {
        return None;
    }
    let text = |s: &str| CString::new(s.replace('\0', "")).unwrap_or_default();
    let titles: Vec<CString> = items.iter().map(|i| text(&i.title)).collect();
    let symbols: Vec<CString> = items.iter().map(|i| text(&i.symbol)).collect();
    let tips: Vec<CString> = items.iter().map(|i| text(&i.tip)).collect();
    let pointers = |v: &[CString]| v.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
    let (t, s, p) = (pointers(&titles), pointers(&symbols), pointers(&tips));
    let checked: Vec<bool> = items.iter().map(|i| i.checked).collect();
    let chosen = unsafe { tb_popup_menu(view, f64::from(x), f64::from(y), t.as_ptr(), s.as_ptr(), p.as_ptr(), checked.as_ptr(), items.len() as i32) };
    usize::try_from(chosen).ok().filter(|i| *i < items.len())
}

/// Who has the keyboard in the window GPUI's view `view` fills.
pub fn keyboard_holder(view: *mut c_void) -> crate::KeyboardHolder {
    use crate::KeyboardHolder;
    if view.is_null() {
        return KeyboardHolder::Window;
    }
    match unsafe { tb_view_keyboard_holder(view) } {
        0 => KeyboardHolder::App,
        1 => KeyboardHolder::Page,
        2 => KeyboardHolder::Window,
        3 => KeyboardHolder::HiddenPage,
        _ => KeyboardHolder::Other,
    }
}

/// Gives GPUI's view `view` its window's keyboard; true if it moved.
pub fn take_keyboard(view: *mut c_void) -> bool {
    !view.is_null() && unsafe { tb_view_take_keyboard(view) }
}

/// A scripted app's typing: `text`'s keys handed to the window GPUI's view
/// `view` is in, which gives them to whoever has its keyboard.
pub fn send_keys(view: *mut c_void, text: &str) {
    let Ok(text) = CString::new(text) else { return };
    if !view.is_null() {
        unsafe { tb_view_send_keys(view, text.as_ptr()) };
    }
}

/// Where Chromium has got to in this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    NotStarted,
    Starting,
    Ready,
    Failed(String),
}

static STATUS: Mutex<Status> = Mutex::new(Status::NotStarted);

pub(crate) fn set_status(status: Status) {
    if let Ok(mut now) = STATUS.lock() {
        *now = status;
    }
}

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or(Status::Failed("the web runtime's state was lost".into()))
}

/// What Chromium needs to find: the bundle the runtime is laid out in, the
/// framework and the first helper in it (all in the data folder,
/// web_runtime::Layout), its profile and its log.
pub struct Launch {
    pub bundle: PathBuf,
    pub framework: PathBuf,
    pub helper: PathBuf,
    pub profile: PathBuf,
    pub log_file: PathBuf,
}

/// The app's bundle id, for the helpers', or None outside a bundle (a cargo
/// build the specs start).
pub fn bundle_identifier() -> Option<String> {
    let id = unsafe { tb_main_bundle_identifier() };
    (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned())
}

thread_local! {
    /// The start, waiting for the run-loop timer the shim sets.
    static PENDING: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

fn text(s: &std::path::Path) -> CefString {
    CefString::from(s.to_string_lossy().as_ref())
}

/// Starts Chromium, once per process. Returns at once: `status()` says when
/// it's ready, which is after the sealed pages' request context is set up too.
pub fn start(launch: Launch) -> Result<(), String> {
    match status() {
        Status::NotStarted => {}
        Status::Failed(why) => return Err(why),
        Status::Starting | Status::Ready => return Ok(()),
    }
    let binary = launch.framework.join("Chromium Embedded Framework");
    let path = CString::new(binary.to_string_lossy().into_owned()).map_err(|_| "the runtime's path has a NUL in it".to_owned())?;
    if unsafe { super::loader_support::cef_load_library(path.as_ptr()) } != 1 {
        let why = "the web runtime couldn't be loaded (its log is web-runtime.log in the data folder)".to_owned();
        set_status(Status::Failed(why.clone()));
        return Err(why);
    }
    // Before anything else: it says which version of the API this side was
    // built against (the versioned API CEF has had since 133).
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    // Before Chromium's pump exists (Ghostex: 2026-06-14 notes).
    unsafe { tb_cef_install_app_hooks() };

    let args = Args::new();
    let mut app = BrowserApp::new();
    if execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut()) >= 0 {
        // CEF took this for one of its own helpers: it never is, as they
        // have their own binary (threadbear-web-helper).
        let why = "the web runtime thought the app was one of its helpers".to_owned();
        set_status(Status::Failed(why.clone()));
        return Err(why);
    }
    std::fs::create_dir_all(&launch.profile).map_err(|e| format!("couldn't make the web runtime's profile: {e}"))?;
    let settings = Settings {
        // Chromium's own sandbox is off. On a Mac it lets a helper read only
        // inside the app bundle macOS started (the browser's real main
        // bundle; CEF's main_bundle_path below doesn't change that), and the
        // runtime lives in the data folder, so every sandboxed helper was
        // refused the framework ("file system sandbox blocked open()") and
        // Chromium gave up. The sealed pages keep their own rules
        // (sealed.rs), an agent's html runs no script at all, and the panels
        // show the sites a patch or Convex names. Turning it on means the
        // framework inside the app (the bundled choice), which is Mike's
        // call (docs/gpui-port.md, "Choice 1").
        no_sandbox: 1,
        // Chromium's "main bundle" is the runtime's (RUNTIME_BUNDLE in
        // web_runtime.rs says why), not the app's: its helpers and the
        // framework are in it, and the sandbox lets them read only that.
        main_bundle_path: text(&launch.bundle),
        browser_subprocess_path: text(&launch.helper),
        framework_dir_path: text(&launch.framework),
        // Chromium's loop nested in GPUI's (the shim says why), not an
        // external pump, and on this thread.
        external_message_pump: 0,
        multi_threaded_message_loop: 0,
        // The transcript's pages are drawn with no window (offscreen.rs).
        windowless_rendering_enabled: 1,
        root_cache_path: text(&launch.profile),
        // The panels' sign-ins live here; the sealed pages have a context of
        // their own that keeps nothing (sealed.rs).
        cache_path: text(&launch.profile.join("panels")),
        persist_session_cookies: 1,
        log_file: text(&launch.log_file),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    set_status(Status::Starting);
    PENDING.with(|p| {
        *p.borrow_mut() = Some(Box::new(move || {
            let mut app = app;
            if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
                set_status(Status::Failed("Chromium didn't start (web-runtime.log in the data folder says why)".into()));
                return;
            }
            run_message_loop();
            // Nothing quits this loop while the app runs.
            eprintln!("threadbear-web: Chromium's message loop returned");
        }))
    });
    unsafe { tb_cef_start_message_loop() };
    Ok(())
}

/// The run-loop timer's callout (the shim).
#[no_mangle]
pub extern "C" fn tb_cef_run_message_loop() {
    if let Some(start) = PENDING.with(|p| p.borrow_mut().take()) {
        start();
    }
}

/// Switches for the browser process. Chromium's helpers inherit what they
/// need from it.
fn browser_switches(command_line: &mut CommandLine) {
    // No Keychain prompt for Chromium's "Safe Storage" key, which an app
    // signed ad hoc would raise again after every build. Cookies are then
    // kept readable by your account only, as the Swift app's WebKit keeps
    // its own (docs/on-your-mac.md).
    // No background requests of Chromium's own (updates of its components,
    // variations, and so on): it goes online for the pages it shows.
    for switch in ["use-mock-keychain", "disable-background-networking", "disable-component-update", "no-pings"] {
        command_line.append_switch(Some(&CefString::from(switch)));
    }
}

wrap_app! {
    struct BrowserApp;

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let browser = process_type.map_or(true, |t| t.to_string().is_empty());
            if let (true, Some(command_line)) = (browser, command_line) {
                browser_switches(command_line);
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserProcess::new())
        }
    }
}

wrap_browser_process_handler! {
    struct BrowserProcess;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            // Ready once the sealed pages' context has its rules on it.
            sealed::create_context();
        }
    }
}
