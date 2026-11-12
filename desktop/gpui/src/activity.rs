// The activity log (docs/guide.md, "The activity log"), as the Swift app's
// ActivityRecorder keeps it: what's clicked, opened, chosen and sent in the
// app, a line each (core's activity.rs), in `<data folder>/activity/<day>.jsonl`,
// so an agent working on Threadbear can see how it's really used (`tb
// activity`). Both apps write the same files and name things the same way;
// this one's lines say `"app": "desktop"`.
//
// Off unless it's switched on in Settings > General (`activityLog` in
// desktop-settings.json): once other people run the app it mustn't look like
// it's watching them. Nothing sends it anywhere, and files go after 30 days.
// It never has what's typed or the words of a message: keys only with ⌘ or ⌃
// (and Escape), and sends and answers by count. A tooltip or menu title can
// name a thread or a folder (a sidebar row's tip is its thread's title), so
// those can be in it.
//
// What's clicked is named by the tooltip of the control under the pointer
// (tips.rs), which every control has, so nothing else in the app needs to say
// what it is.
use crate::app::Threadbear;
use gpui_kit::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use threadbear_core::activity::{self as rules, ActivityEvent};

static ON: AtomicBool = AtomicBool::new(false);

/// The Settings switch, as the app has it now.
pub(crate) fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

struct Recorder {
    app: WeakEntity<Threadbear>,
    main: AnyWindowHandle,
    writer: mpsc::Sender<Write>,
    /// The running build, read at launch.
    build: Option<String>,
    click: Option<PendingClick>,
    /// A shortcut waiting to see whether a menu item's command claims it,
    /// which comes back as that item with its key, rather than twice.
    key: Option<String>,
    place: Option<[Option<String>; 5]>,
    showing: Option<Vec<String>>,
    hover: Option<(String, Instant)>,
    /// Whether one of the app's windows is in front, and the last that was.
    app_active: bool,
    last_window: Option<u64>,
}

/// A press while it's being dispatched: the smallest tip under it so far.
struct PendingClick {
    window: AnyWindowHandle,
    position: Point<Pixels>,
    button: MouseButton,
    clicks: usize,
    modifiers: Modifiers,
    best: Option<(f32, SharedString)>,
}

enum Write {
    Line(String, String),
    Flush(mpsc::Sender<()>),
}

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Recorder) -> R) -> Option<R> {
    RECORDER.with(|r| r.borrow_mut().as_mut().map(f))
}

fn event(kind: &str) -> ActivityEvent {
    ActivityEvent::new(kind, threadbear_core::chrono::Local::now())
}

fn record(mut e: ActivityEvent) {
    if !is_on() {
        return;
    }
    e.app = Some("desktop".into());
    let day = rules::file_name(threadbear_core::chrono::Local::now().date_naive());
    let line = e.line();
    with(|r| {
        let _ = r.writer.send(Write::Line(day, line));
    });
}

/// From the app's start: records nothing unless it's switched on.
pub(crate) fn install(app: WeakEntity<Threadbear>, main: AnyWindowHandle, home: &Path, on: bool, showing: Vec<String>, cx: &mut App) {
    let (writer, lines) = mpsc::channel();
    let folder = home.join("activity");
    let _ = std::thread::Builder::new().name("activity log".into()).spawn(move || write_lines(folder, lines));
    let build = crate::installed::launch().build.as_ref().map(|b| b.short.clone()).filter(|s| !s.is_empty());
    RECORDER.with(|r| {
        *r.borrow_mut() = Some(Recorder { app, main, writer, build: build.clone(), click: None, key: None, place: None, showing: Some(showing.clone()), hover: None, app_active: false, last_window: None });
    });
    ON.store(on, Ordering::Relaxed);
    // A shortcut is noted before it's handled, so a menu item's command can
    // claim it (`menu_chosen`), and written after, if none did.
    cx.intercept_keystrokes(|event, _, _| key_down(&event.keystroke)).detach();
    cx.observe_keystrokes(|_, window, cx| key_handled(window, cx)).detach();
    let mut launch = event("launch");
    launch.version = Some(env!("CARGO_PKG_VERSION").into());
    launch.build = build;
    launch.showing = Some(showing);
    record(launch);
}

/// The Settings switch changed. Every window draws again, so the tips'
/// anchors start (or stop) noting presses.
pub(crate) fn set_on(on: bool, cx: &mut App) {
    ON.store(on, Ordering::Relaxed);
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, _| window.refresh());
    }
}

/// The last line, written before the app goes, with its build.
pub(crate) fn quit() {
    let build = with(|r| r.build.clone()).flatten();
    let mut e = event("quit");
    e.build = build;
    record(e);
    flush();
}

/// Waits for what's queued, for the last line before quitting and for a
/// script reading the file.
pub(crate) fn flush() {
    let (done, wait) = mpsc::channel();
    if with(|r| r.writer.send(Write::Flush(done)).is_ok()) == Some(true) {
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }
}

// What happened.

/// A mouse button pressed in one of the app's windows: from a tip's anchor
/// with its name and size, or from the window's root with none (tips.rs).
/// The first part of a press to arrive starts it; it's written once the
/// press has been dispatched, named by the smallest tip under it.
pub(crate) fn press(e: &MouseDownEvent, claim: Option<(f32, SharedString)>, window: &mut Window, cx: &mut App) {
    if !is_on() {
        return;
    }
    let first = with(|r| {
        let first = r.click.is_none();
        let pending = r.click.get_or_insert_with(|| PendingClick { window: window.window_handle(), position: e.position, button: e.button, clicks: e.click_count, modifiers: e.modifiers, best: None });
        if let Some((area, text)) = claim {
            if pending.best.as_ref().is_none_or(|(best, _)| area < *best) {
                pending.best = Some((area, text));
            }
        }
        first
    });
    if first == Some(true) {
        cx.defer(flush_click);
    }
}

pub(crate) fn flush_click(cx: &mut App) {
    let Some((pending, app, main)) = with(|r| r.click.take().map(|c| (c, r.app.clone(), r.main))).flatten() else { return };
    let mut e = event("click");
    e.target = pending.best.as_ref().and_then(|(_, text)| ActivityEvent::label(text));
    e.window = Some(window_name(pending.window, main, &app, cx));
    if let Some(app) = app.upgrade() {
        if pending.window == main {
            let at = threadbear_dock::Point::new(f64::from(pending.position.x), f64::from(pending.position.y));
            e.panel = app.read(cx).dock.read(cx).controller.panel_at(at, threadbear_dock::DockWindow::Main).map(|p| rules::panel_name(&p));
        }
        e.thread = app.read(cx).selected.clone();
    }
    e.button = match pending.button {
        MouseButton::Left => None,
        MouseButton::Right => Some("right".into()),
        MouseButton::Middle => Some("middle".into()),
        MouseButton::Navigate(NavigationDirection::Back) => Some("button 4".into()),
        MouseButton::Navigate(NavigationDirection::Forward) => Some("button 5".into()),
    };
    if pending.clicks > 1 {
        e.clicks = Some(pending.clicks as u32);
    }
    let m = pending.modifiers;
    let held = rules::modifiers(m.platform, m.control, m.alt, m.shift);
    if !held.is_empty() {
        e.modifiers = Some(held);
    }
    record(e);
}

fn key_down(keystroke: &Keystroke) {
    if !is_on() {
        return;
    }
    let m = &keystroke.modifiers;
    let name = rules::shortcut(&keystroke.key, m.platform, m.control, m.alt, m.shift);
    with(|r| r.key = name);
}

fn key_handled(window: &mut Window, cx: &mut App) {
    let Some(Some(name)) = with(|r| r.key.take()) else { return };
    let Some((app, main)) = with(|r| (r.app.clone(), r.main)) else { return };
    let mut e = event("key");
    e.target = Some(name);
    let here = window_name(window.window_handle(), main, &app, cx);
    if here != "main" {
        e.window = Some(here);
    }
    e.thread = app.upgrade().and_then(|a| a.read(cx).selected.clone());
    record(e);
}

/// A menu item's command, from the menu bar or its key: "Thread › Archive",
/// with the key that chose it if one did. A command no menu has is left to
/// be written as its key.
pub(crate) fn menu_chosen(path: Option<String>, thread: Option<String>) {
    if !is_on() {
        return;
    }
    let Some(path) = path else { return };
    let key = with(|r| r.key.take()).flatten();
    let mut e = event("menu");
    e.target = Some(ActivityEvent::label(&path).unwrap_or_else(|| "untitled item".into()));
    e.key = key;
    e.thread = thread;
    record(e);
}

/// A tooltip came up: the pointer stopped on something to find out what it
/// is. The same one again within a few seconds is one look.
pub(crate) fn hover(text: &str, window: &mut Window, cx: &mut App) {
    if !is_on() {
        return;
    }
    let Some(label) = ActivityEvent::label(text) else { return };
    let now = Instant::now();
    let seen = with(|r| {
        let seen = r.hover.as_ref().is_some_and(|(last, at)| *last == label && now.duration_since(*at) < Duration::from_secs(5));
        if !seen {
            r.hover = Some((label.clone(), now));
        }
        seen
    });
    if seen != Some(false) {
        return;
    }
    let Some((app, main)) = with(|r| (r.app.clone(), r.main)) else { return };
    let mut e = event("hover");
    e.target = Some(label);
    let here = window_name(window.window_handle(), main, &app, cx);
    if here != "main" {
        e.window = Some(here);
    }
    record(e);
}

/// Where the window is now, once per change: the sidebar's view, the thread
/// or sub-agent open, a project's page or a view's own page.
pub(crate) fn view(mode: &str, thread: Option<&str>, subagent: Option<&str>, project: Option<&str>, page: Option<&str>) {
    if !is_on() {
        return;
    }
    let place = [Some(mode), thread, subagent, project, page].map(|s| s.map(str::to_owned));
    let changed = with(|r| {
        let changed = r.place.as_ref() != Some(&place);
        r.place = Some(place);
        changed
    });
    if changed != Some(true) {
        return;
    }
    let mut e = event("view");
    e.mode = Some(mode.to_owned());
    e.thread = thread.map(str::to_owned);
    e.subagent = subagent.map(str::to_owned);
    e.project = project.map(str::to_owned);
    e.page = page.map(str::to_owned);
    record(e);
}

/// What's showing in the dock: the front panel of each group and those in
/// windows of their own. Written when that changes, not for every divider
/// dragged.
pub(crate) fn showing(layout: &threadbear_dock::DockLayout) -> Vec<String> {
    layout.groups().into_iter().map(|g| g.selected).chain(layout.floating.iter().map(|f| f.panel.clone())).map(|p| rules::panel_name(&p)).collect()
}

pub(crate) fn panels(now: Vec<String>) {
    if !is_on() {
        return;
    }
    let changed = with(|r| {
        let changed = r.showing.as_ref() != Some(&now);
        r.showing = Some(now.clone());
        changed
    });
    if changed != Some(true) {
        return;
    }
    let mut e = event("panels");
    e.showing = Some(now);
    record(e);
}

/// A message sent (`thread` none for a new thread's first), by what came
/// with it, never its words.
pub(crate) fn sent(thread: Option<&str>, images: usize, answers: usize, edit: bool) {
    let mut e = event("send");
    e.thread = thread.map(str::to_owned);
    if thread.is_none() {
        e.new = Some(true);
    }
    if edit {
        e.edit = Some(true);
    }
    if images > 0 {
        e.images = Some(images as u32);
    }
    if answers > 0 {
        e.answers = Some(answers as u32);
    }
    record(e);
}

/// A permission prompt or a question answered: allow, allowAlways, deny or
/// answered, as the Swift app's `ApprovalDecision`.
pub(crate) fn answered(thread: &str, decision: &str) {
    let mut e = event("approval");
    e.thread = Some(thread.to_owned());
    e.decision = Some(decision.to_owned());
    record(e);
}

/// One of the app's windows came to the front or went behind (Threadbear's
/// `looking_changed`): the app coming to the front or going behind another
/// app, and another of its windows coming to the front.
pub(crate) fn window_active(window: u64, active: bool, cx: &mut App) {
    let Some(was) = with(|r| r.app_active) else { return };
    if active {
        with(|r| r.app_active = true);
        if !was {
            app_state("active");
        }
        // Only a change of window: the main one coming back with the app is
        // already in "app active".
        let changed = with(|r| r.last_window.replace(window) != Some(window)) == Some(true);
        if changed && is_on() {
            let e = event("window");
            // Named once this update is over: a window's coming to the front
            // is heard inside the app's own update (`looking_changed`), and
            // naming the window reads the app, which GPUI refuses while it's
            // being updated. Opening Settings with the log on crashed the app
            // (Mike, 12 Nov).
            cx.defer(move |cx| {
                let Some((app, main)) = with(|r| (r.app.clone(), r.main)) else { return };
                let handle = cx.windows().into_iter().find(|w| w.window_id().as_u64() == window);
                let mut e = e;
                e.window = Some(handle.map_or_else(|| "window".to_owned(), |h| window_name(h, main, &app, cx)));
                record(e);
            });
        }
        return;
    }
    // Gone behind another app, unless another of its own windows is coming
    // to the front in the same moment (`thread::is_looking` hears of every
    // window, some only ever through it).
    if was && !crate::thread::is_looking() {
        cx.defer(|_| {
            if !crate::thread::is_looking() && with(|r| std::mem::replace(&mut r.app_active, false)) == Some(true) {
                app_state("inactive");
            }
        });
    }
}

fn app_state(state: &str) {
    let mut e = event("app");
    e.state = Some(state.to_owned());
    record(e);
}

/// "main", "settings", or "window" for another of the app's windows.
fn window_name(window: AnyWindowHandle, main: AnyWindowHandle, app: &WeakEntity<Threadbear>, cx: &App) -> String {
    if window == main {
        return "main".into();
    }
    if app.upgrade().and_then(|a| a.read(cx).settings_window.as_ref().map(|(w, _)| *w)) == Some(window) {
        return "settings".into();
    }
    "window".into()
}

// The files.

/// Appends the log's lines on a thread of its own, so a click never waits on
/// the disk. Each line is one write to a file opened for appending, so two
/// apps writing at once (the Swift app and this one) never mix within a line.
fn write_lines(folder: PathBuf, lines: mpsc::Receiver<Write>) {
    let mut day: Option<String> = None;
    let mut file: Option<std::fs::File> = None;
    let mut full = false;
    for message in lines {
        match message {
            Write::Line(name, line) => {
                if day.as_deref() != Some(name.as_str()) {
                    file = open(&folder, &name);
                    day = Some(name.clone());
                    full = false;
                }
                let Some(f) = file.as_mut() else { continue };
                if full {
                    continue;
                }
                if f.metadata().map_or(0, |m| m.len()) > rules::MAX_DAY_BYTES {
                    full = true;
                    eprintln!("threadbear: {name} is past {} MB; nothing more is kept today", rules::MAX_DAY_BYTES / 1_048_576);
                    continue;
                }
                use std::io::Write as _;
                if let Err(e) = f.write_all(format!("{line}\n").as_bytes()) {
                    eprintln!("threadbear: couldn't write the activity log: {e}");
                }
            }
            Write::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn open(folder: &Path, name: &str) -> Option<std::fs::File> {
    // The folder 0700 and each day's file 0600 from the start, through the
    // one place files are made (core's private_file.rs).
    if let Err(e) = threadbear_core::private_file::dir(folder) {
        eprintln!("threadbear: couldn't make {}: {e}", folder.display());
        return None;
    }
    // Time Machine leaves it out; it's only for working on the app.
    exclude_from_backup(folder);
    let names: Vec<String> = std::fs::read_dir(folder).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    for old in rules::expired(&names, threadbear_core::chrono::Local::now().date_naive()) {
        let _ = std::fs::remove_file(folder.join(old));
    }
    match threadbear_core::private_file::append(&folder.join(name)) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("threadbear: couldn't open {name}: {e}");
            None
        }
    }
}

/// The sticky exclusion `tmutil addexclusion` sets (and the Swift app's
/// `isExcludedFromBackup`): this attribute, holding a binary property list of
/// the string "com.apple.backupd".
#[cfg(target_os = "macos")]
fn exclude_from_backup(folder: &Path) {
    use std::os::unix::ffi::OsStrExt as _;
    const NAME: &[u8] = b"com.apple.metadata:com_apple_backup_excludeItem\0";
    const VALUE: [u8; 61] = [
        0x62, 0x70, 0x6C, 0x69, 0x73, 0x74, 0x30, 0x30, 0x5F, 0x10, 0x11, 0x63, 0x6F, 0x6D, 0x2E, 0x61, 0x70, 0x70, 0x6C, 0x65, 0x2E, 0x62, 0x61, 0x63, 0x6B, 0x75, 0x70, 0x64, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1C,
    ];
    let Ok(path) = std::ffi::CString::new(folder.as_os_str().as_bytes()) else { return };
    // SAFETY: a nul-terminated path and name, and a value of the length given.
    unsafe { libc::setxattr(path.as_ptr(), NAME.as_ptr().cast(), VALUE.as_ptr().cast(), VALUE.len(), 0, 0) };
}

#[cfg(not(target_os = "macos"))]
fn exclude_from_backup(_folder: &Path) {}

impl Threadbear {
    /// A menu item's command, from the menu bar or its key (the shell's
    /// actions), noted, then run.
    pub(crate) fn command_chosen(&mut self, command: crate::shell::Command, window: &mut Window, cx: &mut Context<Self>) {
        if is_on() {
            menu_chosen(self.menu_path(command, cx), self.selected.clone());
        }
        self.run_command(command, window, cx);
    }

    /// Settings › General › Activity log.
    pub(crate) fn set_activity_log(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.saved.activity_log = Some(on);
        self.save_settings_soon(cx);
        set_on(on, cx);
        cx.notify();
    }
}

/// The activity log, for a script, as the Swift app's `activity`: `"on"`
/// sets the Settings switch, `"click": "<tip>"` gives the log a press on the
/// control with that tip (without pressing it), `"key": "n"` with
/// `"command": true` presses a key through the app's own dispatch, so a menu
/// item's key comes back as its item, and `"window": "settings"` opens
/// Settings and has its window come to the front as the window server
/// says, through the same path (a scripted app's windows never do). It
/// says whether it's on, how many lines today has and the last few
/// (`"last": N`).
#[cfg(feature = "automation")]
pub(crate) async fn automation(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    let home = app.read_with(cx, |t, _| t.settings.path().parent().map(Path::to_path_buf)).map_err(|e| e.to_string())?.ok_or("no data folder")?;
    let weak = app.clone();
    cx.update_window(window, |_, window, cx| -> Result<(), String> {
        crate::thread::scripted_input(|| {
            let Some(app) = weak.upgrade() else { return Err("the app has gone".to_owned()) };
            if let Some(on) = cmd["on"].as_bool() {
                app.update(cx, |t, cx| t.set_activity_log(on, cx));
            }
            if let Some(tip) = cmd["click"].as_str() {
                let button = if cmd["button"] == "right" { MouseButton::Right } else { MouseButton::Left };
                crate::tips::simulate_press(tip, button, window, cx)?;
                flush_click(cx);
            }
            if let Some(key) = cmd["key"].as_str() {
                let chord = if cmd["command"].as_bool() == Some(true) { format!("secondary-{key}") } else { key.to_owned() };
                use gpui_kit::test::TestWindowExt as _;
                window.press(&chord, cx);
            }
            Ok(())
        })
    })
    .map_err(|e| e.to_string())??;
    if cmd["window"] == "settings" {
        app.update(cx, |t, cx| t.open_settings(None, cx)).map_err(|e| e.to_string())?;
        // The window opens once that update is over.
        cx.background_executor().timer(Duration::from_millis(300)).await;
        let handle = app.read_with(cx, |t, _| t.settings_window.as_ref().map(|(w, _)| *w)).map_err(|e| e.to_string())?.ok_or("Settings didn't open")?;
        // As its window's activation observer does (settings_window.rs):
        // inside the app's own update.
        app.update(cx, |t, cx| t.looking_changed(handle.window_id().as_u64(), true, cx)).map_err(|e| e.to_string())?;
    }
    // A key's line waits for its dispatch to finish, as the Swift app's waits a turn.
    cx.background_executor().timer(Duration::from_millis(300)).await;
    flush();
    let file = home.join("activity").join(rules::file_name(threadbear_core::chrono::Local::now().date_naive()));
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let last = cmd["last"].as_u64().unwrap_or(3) as usize;
    let tail: String = lines[lines.len().saturating_sub(last)..].iter().map(|l| format!(" | {l}")).collect();
    Ok(serde_json::Value::from(format!("ok {} {} lines today{tail}", if is_on() { "on" } else { "off" }, lines.len())))
}
