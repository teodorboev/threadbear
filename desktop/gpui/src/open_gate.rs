// The one way the GPUI app hands a link or a file to the system: a port of the
// Swift app's OpenGate (SECURITY.md, "Your Mac and phone from links and
// files"). The rules are `threadbear_core::open_policy`; this does what they
// say, asks with a real alert, and says why when it won't.
//
// Nothing else in the app calls `open_url` or `reveal_path`:
// desktop/core/tests/open_gate_source.rs fails on one anywhere else, as a
// second opener is how findings 58, 59 and 64 (Daybreak, 9 Nov 2026) got in.
use crate::app::Threadbear;
use gpui_kit::*;
use std::path::Path;
use threadbear_core::open_policy::{self as policy, FileVerdict, LinkKind, OpeningApp, Question};

pub(crate) use threadbear_core::open_policy::Origin;

/// Where a refusal is said: the app's error bar, as the Swift app's
/// `AppModel.errorMessage`.
pub(crate) struct GateNotices(pub WeakEntity<Threadbear>);
impl Global for GateNotices {}

pub(crate) fn install(app: WeakEntity<Threadbear>, cx: &mut App) {
    cx.set_global(GateNotices(app));
}

fn refuse(message: String, cx: &mut App) {
    let Some(app) = cx.try_global::<GateNotices>().and_then(|n| n.0.upgrade()) else { return };
    // Said once the click's update is over: the gate is opened from inside
    // the app's own updates too (the row's Open on Web, the Connectors
    // panel, a project page's links), and updating the app from inside one
    // of those is a double lease, which crashes the app.
    cx.defer(move |cx| app.update(cx, |app, cx| app.show_error(message, cx)));
}

/// The system's own opener. Only this file reaches it.
fn system_open(url: &str, cx: &mut App) {
    if scripted_opens(url) {
        return;
    }
    cx.open_url(url);
}

/// A script's app (the behaviour specs, an agent checking its work) opens
/// nothing for real: its clicks would start apps and browsers on the machine
/// running it. What would have opened is kept for the `opened` command, as
/// the Swift app's scripted runs keep theirs.
#[cfg(feature = "automation")]
fn scripted_opens(what: &str) -> bool {
    if std::env::var_os("THREADBEAR_AUTOMATION").is_none() {
        return false;
    }
    SCRIPTED_OPENS.lock().unwrap_or_else(|e| e.into_inner()).push(what.to_owned());
    true
}

#[cfg(not(feature = "automation"))]
fn scripted_opens(_what: &str) -> bool {
    false
}

#[cfg(feature = "automation")]
static SCRIPTED_OPENS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// What would have opened since this was last asked, oldest first.
#[cfg(feature = "automation")]
pub(crate) fn take_opened() -> Vec<String> {
    std::mem::take(&mut *SCRIPTED_OPENS.lock().unwrap_or_else(|e| e.into_inner()))
}

// ---------------------------------------------------------------- Threadbear's own

/// A file the code itself has just written (the sign-in script), opened in
/// its own app with no questions: Swift's `openOwn`. Never anything a runner,
/// a page, the backend or an agent gave.
pub(crate) fn open_own(path: &Path, cx: &mut App) {
    system_open(&policy::file_url(&path.to_string_lossy()), cx);
}

// ---------------------------------------------------------------- links

/// A web address in the browser. Anything else is refused, said so: for a
/// link the backend or a runner sent that must only ever be a web page (a
/// shared page, the thread on the website).
pub(crate) fn open_web(url: &str, cx: &mut App) -> bool {
    match policy::link_kind(url) {
        LinkKind::Web(web) => {
            system_open(&web, cx);
            true
        }
        _ => {
            refuse(format!("That isn't a web address, so Threadbear didn't open it: {}", policy::shown(url, 300)), cx);
            false
        }
    }
}

/// Any link from a thread, a page, a patch or a pin: a web page in the
/// browser, a file through `open_file` (a relative one from `cwd`), anything
/// else only once Mike says yes to it.
pub(crate) fn open_link(url: &str, origin: Origin, cwd: &str, cx: &mut App) {
    match policy::link_kind(url) {
        LinkKind::Web(web) => system_open(&web, cx),
        LinkKind::File(path) => match policy::absolute(&path, cwd, &crate::files::home_dir()) {
            Some(path) => open_file(&path, origin, cx),
            None => refuse(format!("Threadbear couldn't tell where {} is.", threadbear_core::highlights::last_component(&path)), cx),
        },
        LinkKind::Ask(other) => open_other(&other, cx),
        LinkKind::Refuse => refuse(format!("Threadbear couldn't open that link: {}", policy::shown(url, 300)), cx),
    }
}

/// A web page in one of Chrome's profiles, for a web link's "Open in Chrome:
/// <profile>" (ChromeProfiles.open). A second Chrome started with
/// `--profile-directory` hands the page to the running one and quits, which
/// is the only way in: Chrome has no scripting or URL scheme for picking a
/// profile. Only a web address goes, as it's an argument Chrome acts on
/// (`--some-flag` or a `file:` URL would be Chrome's to run); anything else
/// is refused as `open_web` refuses it, and without Chrome the page goes to
/// the default browser.
pub(crate) fn open_in_chrome_profile(url: &str, directory: &str, cx: &mut App) {
    let LinkKind::Web(web) = policy::link_kind(url) else {
        open_web(url, cx);
        return;
    };
    if scripted_opens(&format!("chrome {directory} {web}")) {
        return;
    }
    if !start_chrome_profile(&web, directory) {
        system_open(&web, cx);
    }
}

#[cfg(target_os = "macos")]
fn start_chrome_profile(web: &str, directory: &str) -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let Some(app) = NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str(threadbear_core::reply_links::CHROME_BUNDLE_ID)) else { return false };
    let Some(app) = app.path().map(|p| p.to_string()) else { return false };
    std::process::Command::new("/usr/bin/open").args(["-na", &app, "--args", &format!("--profile-directory={directory}"), web]).spawn().is_ok()
}

#[cfg(not(target_os = "macos"))]
fn start_chrome_profile(_web: &str, _directory: &str) -> bool {
    false
}

/// The browser web pages open in, with its bundle id (`DefaultBrowser.current()`),
/// for "Open in Google Chrome" and whether Chrome's profiles are worth
/// offering. `default_browser` below is its name alone.
#[cfg(target_os = "macos")]
pub(crate) fn default_browser_app() -> Option<OpeningApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::URLWithString(&NSString::from_str("https://example.com"))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    describe_app(&app)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn default_browser_app() -> Option<OpeningApp> {
    None
}

/// The app the system opens a file in, by name ("Xcode"), for "Open in
/// Xcode" in a file link's menu. Asking says nothing about whether it's
/// safe to open: `open_file` decides that when it's chosen.
pub(crate) fn opening_app(path: &str) -> Option<OpeningApp> {
    app_for(Path::new(path))
}

/// Whether an app with this bundle id is running (NSRunningApplication):
/// Chrome's profiles are offered only while it is.
#[cfg(target_os = "macos")]
pub(crate) fn is_running(bundle_id: &str) -> bool {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id)).count() > 0
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn is_running(_bundle_id: &str) -> bool {
    false
}

/// A link that starts another app: only Threadbear's own isn't handed back,
/// and the rest ask, naming the app.
fn open_other(url: &str, cx: &mut App) {
    if policy::is_own_scheme(url) {
        refuse("Threadbear's own links only open threads from inside a thread.".to_owned(), cx);
        return;
    }
    let scheme = url.split(':').next().unwrap_or("");
    let Some(app) = app_for_link(url) else {
        refuse(format!("Nothing on this computer opens {scheme}: links."), cx);
        return;
    };
    let url = url.to_owned();
    ask(policy::link_question(&url, &app), cx, move |cx| system_open(&url, cx));
}

// ---------------------------------------------------------------- rich text

/// Every rich text view in the app is made here, with its links going
/// through the gate. gpui's own TextView hands a clicked link to the
/// system's opener when it has no handler of its own (Codex review of the
/// 10 Nov fix: a `file://` link to a `.command` in a previewed SKILL.md ran
/// it), so a view made anywhere else would be a way round the gate. A caller
/// with somewhere better for its links (the transcript's `link_clicked`)
/// sets its own `on_link_click` after, which replaces this one.
pub(crate) fn text_view(state: &Entity<gpui_kit::base::TextViewState>, origin: Origin, base: &str) -> gpui_kit::base::TextView {
    let base = base.to_owned();
    gpui_kit::base::TextView::new(state).on_link_click(move |url: &SharedString, event: &ClickEvent, _: &mut Window, cx: &mut App| clicked(url, event, origin, &base, cx))
}

/// The base kit's markdown text made from a string, for text that takes the
/// reading column's style (`rows::reading_style`), as the asks card's
/// paragraphs do.
pub(crate) fn reading_markdown_view(id: impl Into<ElementId>, text: impl Into<SharedString>, origin: Origin, base: &str) -> gpui_kit::base::TextView {
    let base = base.to_owned();
    gpui_kit::base::TextView::markdown(id, text).on_link_click(move |url: &SharedString, event: &ClickEvent, _: &mut Window, cx: &mut App| clicked(url, event, origin, &base, cx))
}

/// A link in one of those clicked. The kit hands over a release of any
/// button on a link: a right click is the menu's, and a probe asking which
/// link is under the pointer (reply_links.rs) only wants to know.
fn clicked(url: &str, event: &ClickEvent, origin: Origin, base: &str, cx: &mut App) {
    if crate::reply_links::noted_by_probe(url) || !crate::reply_links::follows(event) {
        return;
    }
    open_link(url, origin, base, cx);
}

// ---------------------------------------------------------------- files

/// A file in its own app, after its symlinks are followed: a document opens,
/// a program asks first, another Mac's file never opens here. `path` is
/// absolute.
pub(crate) fn open_file(path: &str, origin: Origin, cx: &mut App) {
    match policy::decide_file(path, origin, &app_for) {
        FileVerdict::Open { url, .. } => system_open(&url, cx),
        FileVerdict::Refuse(why) => refuse(why, cx),
        FileVerdict::Ask { question, url, look } => {
            let path = path.to_owned();
            ask(question, cx, move |cx| {
                // What was said yes to, still: not a file swapped in while
                // the question was up.
                if policy::still_same(&path, &look, &app_for) {
                    system_open(&url, cx);
                } else {
                    refuse(format!("{} changed while Threadbear was asking, so it didn't open it.", threadbear_core::highlights::last_component(&path)), cx);
                }
            });
        }
    }
}

// ---------------------------------------------------------------- shown in the app

/// A video or a PDF in the Mac's own view (media_view.rs), in `opening`'s
/// window view: only this Mac's own file, and only a real one of the kind
/// the view is for, after its symlinks are followed (`decide_media`). The
/// view is handed the file the rules opened and checked, and reads that,
/// never the path again (Codex's review, 12 Nov: a file swapped in at the
/// path after the check was what played). The only maker of those views, so
/// nothing reads a film or a PDF any other way, and a PDF's links come back
/// here (`open_link`) rather than going to the system
/// (desktop/core/tests/open_gate_source.rs).
pub(crate) fn show_media(path: &str, origin: Origin, kind: crate::media_view::Kind, opening: crate::media_view::Opening) -> Result<crate::media_view::Media, String> {
    use crate::media_view::Kind;
    use policy::{Media, MediaVerdict};
    let wants = if kind == Kind::PaneDocument { Media::Pdf } else { Media::Video };
    let name = threadbear_core::highlights::last_component(path).to_owned();
    if policy::media_kind(path) != Some(wants) {
        return Err(format!("{name} isn't {}, so Threadbear didn't open it.", if wants == Media::Pdf { "a PDF" } else { "a video" }));
    }
    match policy::decide_media(path, origin) {
        MediaVerdict::Refuse(why) => Err(why),
        MediaVerdict::Show(checked) => open_media(checked, kind, opening).ok_or_else(|| format!("{name} couldn't be read, so it isn't showing.")),
    }
}

#[cfg(target_os = "macos")]
fn open_media(checked: policy::CheckedFile, kind: crate::media_view::Kind, opening: crate::media_view::Opening) -> Option<crate::media_view::Media> {
    use std::os::fd::IntoRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;
    extern "C" {
        fn tb_media_open(parent: *mut std::ffi::c_void, file: i32, length: i64, path: *const std::ffi::c_char, kind: i32, muted: bool, volume: f32) -> *mut std::ffi::c_void;
    }
    let path = std::ffi::CString::new(checked.real.as_os_str().as_bytes()).ok()?;
    // Among them all they're muted, as they all play at once; on the stage,
    // where the last clip was left; in the pane, with its sound.
    let (muted, volume) = match kind {
        crate::media_view::Kind::CellVideo => (true, 1.),
        crate::media_view::Kind::StageVideo => opening.audio.unwrap_or((false, 1.)),
        _ => (false, 1.),
    };
    let length = i64::try_from(checked.len).ok()?;
    // The view owns the open file from here, and closes it, whether or not
    // it's made.
    let file = checked.file.into_raw_fd();
    let view = unsafe { tb_media_open(opening.parent, file, length, path.as_ptr(), kind as i32, muted, volume) };
    // Safety: a view tb_media_open just made, owned by nothing else.
    unsafe { crate::media_view::Media::from_native(view, kind) }
}

/// Elsewhere there's no view for them: they open in their own app.
#[cfg(not(target_os = "macos"))]
fn open_media(_checked: policy::CheckedFile, _kind: crate::media_view::Kind, _opening: crate::media_view::Opening) -> Option<crate::media_view::Media> {
    None
}

/// A file shown in its folder (Finder, or the system's file browser), which
/// shows it without opening it. Another Mac's file isn't on this one.
pub(crate) fn reveal(path: &str, origin: Origin, cx: &mut App) {
    if origin == Origin::OtherMac {
        refuse(format!("{} is on another Mac.", threadbear_core::highlights::last_component(path)), cx);
        return;
    }
    let result = policy::with_local_path(Path::new(path), |_| {
        if !scripted_opens(&format!("reveal {path}")) {
            #[cfg(windows)]
            let real = policy::real_path(Path::new(path))?;
            #[cfg(windows)]
            if real.to_string_lossy().starts_with(r"\\") { return Err(std::io::Error::other("That path can't be shown by the Windows shell.")); }
            #[cfg(not(windows))]
            let real = Path::new(path).to_path_buf();
            cx.reveal_path(policy::system_path(&real));
        }
        Ok(())
    });
    if let Err(error) = result {
        refuse(format!("Couldn't show that file: {error}"), cx);
    }
}

// ---------------------------------------------------------------- asking

/// The question as an alert over the app's window, Cancel first so Return is
/// no; `yes` runs only on the yes button. Asked once the click that led here
/// has finished, as the window is busy with it until then. With no window
/// to ask over, the answer is no.
fn ask(question: Question, cx: &mut App, yes: impl FnOnce(&mut App) + 'static) {
    cx.defer(move |cx| {
        let Some(handle) = cx.active_window().or_else(|| cx.windows().into_iter().next()) else { return };
        let answers = [PromptButton::cancel("Cancel"), PromptButton::new(question.yes.clone())];
        let Ok(answer) = handle.update(cx, |_, window, cx| window.prompt(PromptLevel::Warning, &question.title, Some(&question.detail), &answers, cx)) else { return };
        cx.spawn(async move |cx| {
            if let Ok(1) = answer.await {
                cx.update(yes);
            }
        })
        .detach();
    });
}

// ---------------------------------------------------------------- the system's apps

/// The app the system would open `file` in (`NSWorkspace.urlForApplication`).
#[cfg(target_os = "macos")]
fn app_for(file: &Path) -> Option<OpeningApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    describe_app(&app)
}

/// Elsewhere nothing says, and the extension rules stand alone.
#[cfg(not(target_os = "macos"))]
fn app_for(_file: &Path) -> Option<OpeningApp> {
    None
}

/// The app a link would start, by name, or None if nothing opens it.
#[cfg(target_os = "macos")]
fn app_for_link(link: &str) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::URLWithString(&NSString::from_str(link))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    describe_app(&app).map(|a| a.name)
}

/// Elsewhere the system can't be asked, so the link's scheme stands for its app.
#[cfg(not(target_os = "macos"))]
fn app_for_link(link: &str) -> Option<String> {
    Some(format!("the app for {}: links", link.split(':').next().unwrap_or("")))
}

/// The app web links open in ("Google Chrome"), for naming what `open_web`
/// will open: Settings › Phone & browser names a browser's sign-in after it
/// (`DefaultBrowser`). Only asks; nothing opens.
pub(crate) fn default_browser() -> Option<String> {
    app_for_link("https://example.com").filter(|_| cfg!(target_os = "macos"))
}

#[cfg(target_os = "macos")]
fn describe_app(app: &objc2_foundation::NSURL) -> Option<OpeningApp> {
    use objc2_foundation::NSBundle;
    let path = std::path::PathBuf::from(app.path()?.to_string());
    let bundle_id = NSBundle::bundleWithURL(app).and_then(|b| b.bundleIdentifier()).map(|s| s.to_string());
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "its app".to_owned());
    Some(OpeningApp { path, bundle_id, name })
}
