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

/// A web page from a thread, a pin or a panel, in the browser, unless an
/// agent already has it open through BrowserBear: then that tab comes to
/// Mike instead of a second one opening (agent_browser.rs, `handoff`; the
/// Swift app's `openWebPage`). Anything but a web address is refused as
/// `open_web` refuses it.
pub(crate) fn open_web_page(url: &str, cx: &mut App) {
    let LinkKind::Web(web) = policy::link_kind(url) else {
        open_web(url, cx);
        return;
    };
    if !crate::agent_browser::may_hand_off(&web) {
        system_open(&web, cx);
        return;
    }
    cx.spawn(async move |cx| {
        let asked = web.clone();
        // Up to 4 s on BrowserBear's socket, off the main thread.
        let handed = cx.background_executor().spawn(async move { crate::agent_browser::handoff(&asked) }).await;
        if !handed {
            cx.update(|cx| system_open(&web, cx));
        }
    })
    .detach();
}

/// Any link from a thread, a page, a patch or a pin: a web page in the
/// browser (or an agent's tab already on it), a file through `open_file` (a relative one from `cwd`), anything
/// else only once Mike says yes to it.
pub(crate) fn open_link(url: &str, origin: Origin, cwd: &str, cx: &mut App) {
    match policy::link_kind(url) {
        LinkKind::Web(web) => open_web_page(&web, cx),
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
    #[cfg(target_os = "macos")]
    {
        // The Chrome that's running (the profiles are only offered while it
        // is), never whichever bundle Launch Services has for Chrome's id:
        // any folder's .app can claim that id, and `open -na` starts the
        // bundle it's given (Daybreak 112, 13 Nov 2026). Its signature is
        // checked off the main thread (a quarter of a second), with a note
        // up meanwhile, and anything but Google's own Chrome isn't started.
        let Some(chrome) = running_chrome().filter(|p| policy::real_path(Path::new(p)).is_ok_and(|r| installed_where_apps_are(&r))) else {
            system_open(&web, cx);
            return;
        };
        let Some(app) = cx.try_global::<GateNotices>().and_then(|n| n.0.upgrade()) else { return };
        let directory = directory.to_owned();
        cx.defer(move |cx| {
            if !app.update(cx, |app, cx| app.begin_note("opening-chrome", "Opening Chrome…", cx)) { return; }
            cx.spawn(async move |cx| {
                let page = web.clone();
                let result = cx.background_executor().spawn(async move { start_chrome_profile(&chrome, &page, &directory) }).await;
                cx.update(|cx| {
                    app.update(cx, |app, cx| app.end_note("opening-chrome", cx));
                    if let Err(why) = result { refuse(why, cx) }
                });
            }).detach();
        });
        return;
    }
    #[cfg(not(target_os = "macos"))]
    system_open(&web, cx);
}

/// The running Chrome's bundle, if there is one.
#[cfg(target_os = "macos")]
fn running_chrome() -> Option<String> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(threadbear_core::reply_links::CHROME_BUNDLE_ID));
    (0..running.count()).find_map(|i| running.objectAtIndex(i).bundleURL().and_then(|u| u.path()).map(|p| p.to_string()))
}

/// What Google's Chrome is signed as: Apple's anchor for a Developer ID,
/// Chrome's own id and Google's team. A copy with Chrome's id but anything
/// else inside fails it.
/// Where an app this gate starts by itself may be: the Mac's own folders
/// for apps, which an agent confined to a project can't write. Chrome in
/// ~/Applications goes to the default browser without its profile.
#[cfg(target_os = "macos")]
const INSTALLED_APPS: &[&str] = &["/Applications/", "/System/Applications/", "/Library/Developer/"];

/// Whether a real (symlinks followed) path is under `INSTALLED_APPS`.
#[cfg(target_os = "macos")]
fn installed_where_apps_are(real: &Path) -> bool {
    INSTALLED_APPS.iter().any(|root| real.starts_with(root))
}

#[cfg(target_os = "macos")]
const GOOGLE_CHROME: &str = "anchor apple generic and identifier \"com.google.Chrome\" and certificate leaf[subject.OU] = \"EQHXZ8M8AV\"";

#[cfg(target_os = "macos")]
fn start_chrome_profile(path: &str, web: &str, directory: &str) -> Result<(), String> {
    let real = signed_as(path, GOOGLE_CHROME).map_err(|_| "The Chrome that's running isn't Google's Chrome, so Threadbear didn't open the page in it.".to_owned())?;
    std::process::Command::new("/usr/bin/open").arg("-na").arg(&real).args(["--args", &format!("--profile-directory={directory}"), web]).spawn().map(|_| ()).map_err(|e| format!("Chrome didn't open: {e}"))
}

/// The application at `path`, symlinks followed, if it's installed where
/// apps are (`INSTALLED_APPS`) and its code signature is valid and meets
/// `requirement` (`codesign -R`), so a fixed button or menu never starts a
/// bundle that only borrowed another app's id. The place matters too: the
/// check and `open` each read the bundle at its path, so a signed copy in a
/// project folder could be swapped between them by whatever writes there
/// (Codex's review of the Daybreak 112 fix).
#[cfg(target_os = "macos")]
fn signed_as(path: &str, requirement: &str) -> Result<std::path::PathBuf, ()> {
    let real = policy::real_path(Path::new(path)).map_err(|_| ())?;
    if !installed_where_apps_are(&real) { return Err(()); }
    let signed = std::process::Command::new("/usr/bin/codesign")
        // Not --strict: that also refuses extended attributes on the bundle
        // ("detritus"), which a real Chrome on a real Mac can have (Tinker
        // Desk's did), and says nothing about who made the code. The
        // signature, the sealed resources and the requirement are checked.
        .args(["--verify", &format!("-R={requirement}")])
        .arg(&real).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    if signed.is_ok_and(|s| s.success()) { Ok(real) } else { Err(()) }
}

/// The picture panel's fixed destination, as in Swift's ImagePanel. Nothing
/// from a patch can choose a program or supply arguments here.
pub(crate) fn open_simulator(cx: &mut App) {
    if scripted_opens("application com.apple.iphonesimulator") { return; }
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::NSString;
        let url = NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str("com.apple.iphonesimulator"));
        let Some(path) = url.and_then(|u| u.path()).map(|p| p.to_string()) else { refuse("Simulator isn't installed on this Mac.".into(), cx); return };
        let Some(app) = cx.try_global::<GateNotices>().and_then(|n| n.0.upgrade()) else { return };
        cx.defer(move |cx| {
            if !app.update(cx, |app, cx| app.begin_note("opening-simulator", "Opening Simulator…", cx)) { return; }
            cx.spawn(async move |cx| {
                let result = cx.background_executor().spawn(async move { apple_simulator(&path) }).await;
                cx.update(|cx| {
                    app.update(cx, |app, cx| app.end_note("opening-simulator", cx));
                    match result {
                        Ok(path) => system_open(&policy::file_url(&path), cx),
                        Err(why) => refuse(why, cx),
                    }
                });
            }).detach();
        });
        return;
    }
    #[cfg(not(target_os = "macos"))]
    refuse("Simulator is only available on macOS.".to_owned(), cx);
}

#[cfg(target_os = "macos")]
fn apple_simulator(path: &str) -> Result<String, String> {
    if policy::real_path(Path::new(path)).is_err() { return Err("Simulator isn't there any more.".to_owned()); }
    // Any .app on this Mac can be registered with Launch Services (Finder
    // and Spotlight do it on sight). A copied bundle id must never turn this
    // fixed button into a way to run that application without the gate's
    // usual question.
    let real = signed_as(path, "anchor apple and identifier \"com.apple.iphonesimulator\"").map_err(|_| "The registered Simulator isn't Apple's Simulator, so Threadbear didn't open it.".to_owned())?;
    Ok(real.to_string_lossy().into_owned())
}

#[cfg(all(test, target_os = "macos"))]
mod simulator_tests {
    #[test]
    fn the_selected_xcodes_simulator_passes_the_apple_signature_check() {
        let selected = std::process::Command::new("/usr/bin/xcode-select").arg("-p").output().unwrap();
        let developer = String::from_utf8_lossy(&selected.stdout);
        let simulator = std::path::Path::new(developer.trim()).join("Applications/Simulator.app");
        // The command-line-tools-only Mac has no Simulator. GitHub's Mac
        // check has Xcode and exercises its real nested app signature.
        if !simulator.exists() { return; }
        assert!(super::apple_simulator(&simulator.to_string_lossy()).is_ok(), "Apple's installed Simulator was refused");
    }

    #[test]
    fn a_copied_simulator_bundle_id_does_not_allow_an_unsigned_application() {
        let folder = std::env::temp_dir().join(format!("tb-unsigned-simulator-{}.app", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        threadbear_core::private_file::dir(&folder.join("Contents")).unwrap();
        threadbear_core::private_file::write(&folder.join("Contents/Info.plist"), br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.apple.iphonesimulator</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#).unwrap();
        let result = super::apple_simulator(&folder.to_string_lossy());
        let _ = std::fs::remove_dir_all(folder);
        assert!(result.is_err(), "An unsigned app with Simulator's id passed: {result:?}");
    }
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

/// A small, read-only raster for a local PDF or video hover card. Called on
/// the background executor; the asset loader serializes decoders. Failure keeps the kind icon.
#[cfg(target_os = "macos")]
pub(crate) fn media_thumbnail(path: &str, origin: Origin, expected: Option<policy::MediaVersion>) -> Option<Vec<u8>> {
    use std::os::fd::IntoRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;
    let expected = expected?;
    let kind = policy::media_kind(path)?;
    if !matches!(kind, policy::Media::Pdf | policy::Media::Video) { return None }
    let pdf = kind == policy::Media::Pdf;
    let policy::MediaVerdict::Show(checked) = policy::decide_media(path, origin) else { return None };
    if policy::media_version(&checked.file.metadata().ok()?) != Some(expected) { return None }
    if pdf && checked.len > 16 * 1024 * 1024 { return None }
    let name = std::ffi::CString::new(checked.real.as_os_str().as_bytes()).ok()?;
    let length = i64::try_from(checked.len).ok()?;
    extern "C" {
        fn tb_media_thumbnail(file: i32, length: i64, name: *const std::ffi::c_char, pdf: bool, expected: *const policy::MediaVersion, out: *mut u8, capacity: usize) -> usize;
    }
    let mut png = vec![0; 2 * 1024 * 1024];
    // The native function owns the descriptor on every path, reopens no original path,
    // caps its raster at 552×340 and its system thumbnail request at five seconds.
    let count = unsafe { tb_media_thumbnail(checked.file.into_raw_fd(), length, name.as_ptr(), pdf, &expected, png.as_mut_ptr(), png.len()) };
    if count == 0 || count > png.len() { return None }
    png.truncate(count);
    Some(png)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn media_thumbnail(_path: &str, _origin: Origin, _expected: Option<policy::MediaVersion>) -> Option<Vec<u8>> { None }

// ---------------------------------------------------------------- shown in the app

/// The gate's checked icon input: an icns or PDF file, open, with its length.
/// An application's icon is its own icns file, found and read like any other
/// (`application_icon`).
#[cfg(target_os = "macos")]
pub(crate) struct CheckedIcon(std::fs::File, u64);

/// Resolve and read only on a worker, including for application bundles on
/// a disk that may be slow. Remote paths never name a file on this Mac.
#[cfg(target_os = "macos")]
pub(crate) fn read_icon(path: &Path, origin: Origin) -> Result<CheckedIcon, std::io::Error> {
    if origin == Origin::OtherMac || !path.is_absolute() {
        return Err(std::io::Error::other("That icon isn't on this Mac"));
    }
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let wanted = ext(path);
    let real = policy::real_path(path)?;
    if ext(&real) != wanted || !matches!(wanted.as_str(), "app" | "pdf" | "icns") {
        return Err(std::io::Error::other("That isn't a Mac icon"));
    }
    if wanted == "app" {
        if !real.is_dir() { return Err(std::io::Error::other("That isn't an application bundle")); }
        return application_icon(&real);
    }
    checked_icon_file(&real, &wanted)
}

/// An application's icon, read as a file from inside its bundle: the icns
/// its Info.plist names (CFBundleIconFile) in Contents/Resources. Never
/// NSWorkspace's icon for it, which can register the bundle with Launch
/// Services, so a folder's .app claiming Chrome's id could be the one a
/// launch by that id starts (Daybreak 112, 13 Nov 2026). An app whose icon
/// is only in an asset catalog shows its patch's symbol instead.
#[cfg(target_os = "macos")]
fn application_icon(bundle: &Path) -> Result<CheckedIcon, std::io::Error> {
    extern "C" {
        fn tb_bundle_icon_file(plist: *const u8, length: usize, output: *mut u8, capacity: usize) -> isize;
    }
    let inside = |p: &Path| -> Result<std::path::PathBuf, std::io::Error> {
        let real = policy::real_path(p)?;
        if real.starts_with(bundle) && real != bundle { Ok(real) } else { Err(std::io::Error::other("That application's icon is outside it")) }
    };
    const PLIST_BYTES: usize = 1024 * 1024;
    let plist = policy::local_read(&inside(&bundle.join("Contents/Info.plist"))?, PLIST_BYTES + 1)?;
    if plist.len() > PLIST_BYTES { return Err(std::io::Error::other("That application's Info.plist is too big")); }
    let mut name = [0u8; 256];
    let length = unsafe { tb_bundle_icon_file(plist.as_ptr(), plist.len(), name.as_mut_ptr(), name.len()) };
    let name = usize::try_from(length).ok().and_then(|n| std::str::from_utf8(&name[..n]).ok()).unwrap_or_default();
    // A file name in Resources, nothing that climbs or names a folder.
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', '\0']) {
        return Err(std::io::Error::other("That application names no icon file"));
    }
    let file = if name.to_ascii_lowercase().ends_with(".icns") { name.to_owned() } else { format!("{name}.icns") };
    let real = inside(&bundle.join("Contents/Resources").join(file))?;
    checked_icon_file(&real, "icns")
}

/// `real`, opened, if it's a small regular file of the kind `wanted` says
/// (`pdf` or `icns`) by its first bytes.
#[cfg(target_os = "macos")]
fn checked_icon_file(real: &Path, wanted: &str) -> Result<CheckedIcon, std::io::Error> {
    let file = policy::local_open(real)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
        return Err(std::io::Error::other("That icon isn't a small regular file"));
    }
    let mut head = [0u8; 8];
    let got = std::os::unix::fs::FileExt::read_at(&file, &mut head, 0)?;
    let matches = if wanted == "pdf" {
        policy::looks_like(policy::Media::Pdf, &head[..got], meta.len())
    } else {
        got == 8 && head.starts_with(b"icns") && u32::from_be_bytes(head[4..8].try_into().unwrap()) as u64 == meta.len()
    };
    if !matches { return Err(std::io::Error::other("That file isn't the icon it names")); }
    Ok(CheckedIcon(file, meta.len()))
}

/// PNG bytes for a checked patch icon, on a worker (ImageIO and
/// CoreGraphics). AppKit never sees the application.
#[cfg(target_os = "macos")]
pub(crate) fn icon_png(icon: CheckedIcon) -> Result<Vec<u8>, std::io::Error> {
    use std::os::fd::AsRawFd as _;
    extern "C" {
        fn tb_file_icon_png(fd: i32, length: i64, output: *mut *mut std::ffi::c_void) -> i32;
        fn free(data: *mut std::ffi::c_void);
    }
    let mut output = std::ptr::null_mut();
    let CheckedIcon(file, length) = icon;
    let length = unsafe { tb_file_icon_png(file.as_raw_fd(), length as i64, &mut output) };
    if output.is_null() || length <= 0 { return Err(std::io::Error::other("That icon couldn't be decoded")); }
    // AppKit hands over a malloc copy, so Rust owns neither an Objective-C
    // object nor the allocation after this copy and free.
    let bytes = unsafe { std::slice::from_raw_parts(output.cast::<u8>(), length as usize).to_vec() };
    unsafe { free(output); }
    Ok(bytes)
}

#[cfg(all(test, target_os = "macos"))]
#[path = "open_gate/icon_tests.rs"]
mod icon_tests;

/// A video, PDF or document thumbnail in the Mac's own view (media_view.rs), in `opening`'s
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
    let wants = match kind { Kind::PaneDocument => Media::Pdf, Kind::PaneQuickLook => Media::Office, _ => Media::Video };
    let name = threadbear_core::highlights::last_component(path).to_owned();
    if policy::media_kind(path) != Some(wants) {
        let what = match wants { Media::Pdf => "a PDF", Media::Office => "an Office or iWork document", Media::Video => "a video" };
        return Err(format!("{name} isn't {what}, so Threadbear didn't open it."));
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
