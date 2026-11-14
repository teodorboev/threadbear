// The installed app keeps itself on main, as the Swift app's AppUpdater does
// (core's app_update.rs has the rules). Twenty seconds after launch, every
// five minutes, and within seconds of main moving, it reads main's tree of
// desktop/ in the checkout it was built from, and when that differs from what's
// running it waits for ten quiet minutes before building the latest main into
// ~/Applications/.Threadbear GPUI.app.next
// (desktop/scripts/stage-update.sh, from the bundle). The Rust release build
// takes every core for minutes, so the script builds on a Mac among
// runner.json's updateBuildHosts and only the binaries come back. Check hosts
// never build it: what comes back is what runs here, and they're trusted only
// with tests (Daybreak 108). With check hosts but no update build hosts it
// builds nowhere and the Updates panel says so; with neither (someone else's
// copy) it builds here.
//
// Quitting swaps the staged bundle in, so the next launch is the new app; a
// launch that finds one staged (the app was killed rather than quit) swaps it
// in and starts again. Restart to Update, in the app menu and the Updates
// panel, does it now, and the app does it by itself once nothing typed would
// be lost and nothing under way cut off, then puts back the list and the
// thread he had open: straight away while Mike's in another app, and with the
// app in front once he hasn't touched it for five seconds.
//
// With the window on screen, the restart is a handover (handover.rs, as the
// Swift app's Handover.swift): the new build starts under the old window and
// takes its place once it's drawn, so the window never goes, and each
// thread's scroll place comes with it (`restore_state`). Mike keeps this app
// in front, so its updates never went in until he clicked (12 Nov); he'd
// rather it went in by itself, and with the handover it goes unseen.
use crate::app::Threadbear;
use crate::installed;
use gpui_kit::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// The Updates panel's reason, after "The app isn't following main: ".
const NO_BUILDER: &str = "no Mac is trusted to build it. Check hosts only run tests, so add one you trust with what this Mac runs to updateBuildHosts in runner.json";
use std::time::Duration;
use threadbear_core::app_update::{self as rules, Action, BuildInfo};

const FIRST_LOOK: Duration = Duration::from_secs(20);
const LOOK_EVERY: Duration = Duration::from_secs(5 * 60);
/// How often a staged build is weighed against what's going on: a few
/// comparisons, so often, to go soon after Mike leaves the app.
const RESTART_LOOK: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(crate) struct AppUpdater {
    /// The running bundle's build, or none when there's nothing to follow.
    pub(crate) running: Option<BuildInfo>,
    /// The bundle waiting to be swapped in.
    pub(crate) staged: Option<BuildInfo>,
    /// Main's tree being built, and the one whose build failed (not tried
    /// again until main moves on).
    pub(crate) building: Option<String>,
    pub(crate) failed: Option<String>,
    quiet_main: rules::BuildDebounce,
    settling: bool,
    /// The last build found no Mac to build on; the next look tries again.
    pub(crate) waiting_for_host: bool,
    /// The last build found no Mac trusted to build it (stage-update.sh's 5:
    /// check hosts but no updateBuildHosts), so Mike has to name one; the
    /// next look tries again, so naming one is picked up without a restart.
    pub(crate) no_builder: bool,
    /// Why it isn't following main: for the log, and the Updates panel when
    /// it's the installed app.
    pub(crate) disabled: Option<String>,
    restarting: bool,
    /// The restart under way is one the app chose by itself, not Restart to
    /// Update: a handover's go waits for what that would have waited for.
    pub(crate) auto_restart: bool,
    /// What the staged build's restart waits on, as the Updates panel says
    /// it, set at each once-a-second look (`consider_auto_restart`).
    pub(crate) wait: Option<rules::RestartWait>,
    /// The same hold since when, and whether the log has it: one that lasts
    /// a minute goes in the log once, so a restart that never came says why.
    held_since: Option<(String, std::time::Instant, bool)>,
    build_pid: Option<u32>,
    soon: Option<Task<()>>,
    watch: Option<crate::file_watch::FolderWatch>,
    tasks: Vec<Task<()>>,
    /// A made-up update for a scripted app's pictures (`updates` with
    /// `preview`): Restart to Update does nothing with it.
    preview: bool,
}

fn install_dir() -> PathBuf {
    installed::install_dir()
}

fn beside(name: &str) -> PathBuf {
    install_dir().parent().map_or_else(|| PathBuf::from(name), |dir| dir.join(name))
}

fn next_dir() -> PathBuf {
    beside(".Threadbear GPUI.app.next")
}

/// The staged build, once one whose builder isn't trusted has gone
/// (`drop_untrusted_staged`).
fn staged_build() -> Option<BuildInfo> {
    if !drop_untrusted_staged() {
        return None;
    }
    BuildInfo::read(&next_dir())
}

/// A staged bundle goes in only if it was built here or on a host in
/// runner.json's updateBuildHosts now: never one a check host built, or one
/// staged before bundles said where they were built (Daybreak 108).
/// False when one that isn't trusted is still there: nothing goes in then.
fn drop_untrusted_staged() -> bool {
    let runner_json = threadbear_client::config::RunnerConfig::from_env().home.join("runner.json");
    match rules::drop_untrusted_staged(&next_dir(), &runner_json) {
        Ok(None) => true,
        Ok(Some(why)) => {
            note(&why);
            true
        }
        Err(why) => {
            note(&why);
            false
        }
    }
}

fn old_dir() -> PathBuf {
    beside(".Threadbear GPUI.app.old")
}

/// The note a swap keeps while it happens (core's `swap`).
fn swapping_note() -> PathBuf {
    beside(".Threadbear GPUI.app.swapping")
}

/// The update build's output, for Show the Log.
pub(crate) fn log_path() -> PathBuf {
    installed::home_dir().join("Library").join("Logs").join("threadbear-desktop-update.log")
}

/// The script in the running bundle, so the app runs the copy it was built with.
fn script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let script = threadbear_core::app_update::bundle_of(&exe)?.join("Contents").join("Resources").join("stage-update.sh");
    script.exists().then_some(script)
}

/// Why the swap at launch failed, if it did: the app doesn't follow main for
/// the rest of this run, as it would only stage the build again and restart
/// into the same failure, over and over.
static LAUNCH_SWAP_FAILED: OnceLock<String> = OnceLock::new();

/// A build staged while the app last ran (it was killed rather than quit, or
/// quitting couldn't swap it): put in place, and the new app opened once
/// this one has gone. First thing in main(); true means exit now.
pub(crate) fn apply_staged_at_launch() -> bool {
    let launch = installed::launch();
    if !launch.installed {
        return false;
    }
    if !drop_untrusted_staged() {
        return false;
    }
    match rules::apply_staged(&install_dir(), &next_dir(), &old_dir(), &swapping_note(), launch.build.as_ref()) {
        Ok(Some(staged)) => {
            note(&format!("swapped in {} at launch", staged.short));
            relaunch_after_exit(&install_dir(), false);
            true
        }
        Ok(None) => false,
        Err(e) => {
            note(&e);
            let _ = LAUNCH_SWAP_FAILED.set(e);
            false
        }
    }
}

/// What the installed app was built from, in hello's `build`, so the runner
/// can tell a thread waiting for the app to restart when it's back on a new
/// one (docs/protocol.md, "App restarts"). Any other launch sends none.
pub(crate) fn announce_build() {
    let launch = installed::launch();
    if let Some(build) = launch.build.as_ref().filter(|b| launch.installed && !b.app_tree.is_empty()) {
        threadbear_client::runner::set_hello_build(json!({ "commit": build.commit, "short": build.short, "tree": build.app_tree }));
    }
}

/// Opens the installed app again once this process has gone: a shell that
/// waits for this pid, then `open`, `-g` for a restart Mike didn't ask for
/// while he's in another app. Our own bundle, from its fixed place, with
/// every Threadbear setting this run was given (its data folder, its install
/// place, Sentry off): `open` starts it with LaunchServices' environment, not
/// ours, so one left out came back as if never set (Codex's review, 12 Nov:
/// THREADBEAR_SENTRY=off and THREADBEAR_DATA_DIR were lost). Never a thread's
/// identity, which the app may have inherited from a thread it was started
/// in, nor the scripting switch, as a scripted app never restarts itself.
fn relaunch_after_exit(bundle: &Path, in_background: bool) {
    let mut open: Vec<std::ffi::OsString> = Vec::new();
    if in_background {
        open.push("-g".into());
    }
    for (key, value) in settings_to_pass_on() {
        let mut pair = std::ffi::OsString::from(format!("{key}="));
        pair.push(value);
        open.extend(["--env".into(), pair]);
    }
    open.push(bundle.into());
    let wait = format!("while /bin/kill -0 {} 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open \"$@\"", std::process::id());
    let started = Command::new("/bin/sh").arg("-c").arg(wait).arg("sh").args(open).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Err(e) = started {
        note(&format!("couldn't start the relaunch: {e}"));
    }
}

/// The Threadbear settings this run was given, for `open` to pass on to the
/// next (`relaunch_after_exit`, and handover.rs's second copy).
pub(crate) fn settings_to_pass_on() -> Vec<(String, std::ffi::OsString)> {
    let mut settings: Vec<(String, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value)))
        .filter(|(key, _)| (key.starts_with("THREADBEAR_") || key.starts_with("THREADBARE_")) && !["_THREAD_ID", "_THREAD_KEY", "_AUTOMATION"].iter().any(|s| key.ends_with(s)))
        .collect();
    settings.sort();
    settings
}

/// The installed app opened again once this one has gone, in front: a
/// handover whose new app went before taking over (handover.rs).
pub(crate) fn relaunch_installed() {
    relaunch_after_exit(&install_dir(), false);
}

/// The installed app's build, if it can follow main, or why it can't.
fn followable() -> Result<BuildInfo, String> {
    let launch = installed::launch();
    if !launch.installed {
        return Err("not the installed app".into());
    }
    // Its build.json and the update script came with desktop/scripts/
    // make-bundle.sh: an app installed before then has neither.
    const REINSTALL: &str = "it was installed before it could. Install it again with desktop/scripts/install-app.sh";
    let info = launch.build.clone().filter(|b| !b.app_tree.is_empty()).ok_or(REINSTALL)?;
    script().ok_or(REINSTALL)?;
    if !Path::new(&info.checkout).join(".git").exists() {
        return Err(format!("the checkout it was installed from, {}, is gone", info.checkout));
    }
    if let Some(e) = LAUNCH_SWAP_FAILED.get() {
        return Err(format!("its new version couldn't go in ({e})"));
    }
    Ok(info)
}

/// Main's tree in the checkout, or none if git can't say.
fn main_tree(checkout: &str) -> Option<String> {
    let out = Command::new("/usr/bin/git").args(rules::rev_parse_arguments(checkout)).stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        note(&format!("git rev-parse main:desktop failed in {checkout}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        return None;
    }
    Some(rules::tree_from_rev_parse(&String::from_utf8_lossy(&out.stdout))).filter(|t| !t.is_empty())
}

fn open_log() -> Option<std::fs::File> {
    let path = log_path();
    // A build here writes all of cargo's output: past 2 MB the old log goes
    // to `.1`, so it never grows without end.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2 << 20) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    // Added to by the app and by the script's output both (core's
    // private_file.rs, as every file the app writes is).
    threadbear_core::private_file::append(&path).ok()
}

/// A line in the update log, with the time, as well as on stderr: the
/// installed app's stderr goes nowhere, and Show the Log is where Mike looks.
fn note(line: &str) {
    eprintln!("threadbear: {line}");
    if let Some(mut log) = open_log() {
        use std::io::Write as _;
        let at = threadbear_core::inbox::iso_time(crate::now_ms());
        let _ = writeln!(log, "[{at}] app: {line}");
    }
}

/// What the app's windows have up that a restart would close, read once a
/// second for `consider_auto_restart`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowsNow {
    /// A dialog, sheet, prompt or menu in one of them, or one of AppKit's own
    /// panels (an alert, a file picker) up over them.
    pub(crate) held: bool,
    /// A window other than the main one has focus with a field being typed in.
    pub(crate) other_window_input: bool,
}

pub(crate) fn windows_now(main: AnyWindowHandle, cx: &mut App) -> WindowsNow {
    use gpui_kit::component::WindowExt as _;
    let mut now = WindowsNow { held: native_panel_up(), other_window_input: false };
    for handle in cx.windows() {
        let looked = handle.update(cx, |_, window, cx| {
            // The kit's menus take the keys while they're open.
            let menu = window.context_stack().iter().any(|c| c.contains("PopupMenu"));
            let held = menu || window.has_active_dialog(cx) || window.has_active_sheet(cx) || window.has_active_prompt();
            (held, window.is_window_active() && window.has_focused_input(cx))
        });
        // A window being drawn right now can't be read: count it as busy.
        let (held, typing) = looked.unwrap_or((true, false));
        now.held |= held;
        now.other_window_input |= typing && handle.window_id() != main.window_id();
    }
    now
}

/// An alert running (`runModal`), or a file picker or a prompt's sheet with
/// the keys: each is a panel AppKit made, and the app's own windows aren't.
#[cfg(target_os = "macos")]
fn native_panel_up() -> bool {
    use objc2::runtime::NSObjectProtocol as _;
    use objc2::{ClassType as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSPanel};
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    app.modalWindow().is_some() || app.keyWindow().is_some_and(|w| w.isKindOfClass(NSPanel::class()))
}

#[cfg(not(target_os = "macos"))]
fn native_panel_up() -> bool {
    false
}

/// The app is in front (`NSApp.isActive`, as the Swift app asks): not each
/// window's activation (`thread::is_looking`), which goes when a file picker
/// or an alert takes the keys, though the app's still in front under it.
#[cfg(target_os = "macos")]
fn app_active() -> bool {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return true };
    objc2_app_kit::NSApplication::sharedApplication(mtm).isActive()
}

#[cfg(not(target_os = "macos"))]
fn app_active() -> bool {
    crate::thread::is_looking()
}

/// Seconds since the last key, click, pointer move or scroll anywhere on the
/// Mac (the hang watch's own question), which with the app in front is input
/// to it. Covers every window of the app, Settings and the review window too.
#[cfg(target_os = "macos")]
pub(crate) fn idle_seconds() -> f64 {
    crate::hang_watch::idle_seconds().unwrap_or(0.)
}

#[cfg(windows)]
pub(crate) fn idle_seconds() -> f64 {
    #[repr(C)]
    struct LastInput { size: u32, tick: u32 }
    #[link(name = "user32")]
    extern "system" { fn GetLastInputInfo(info: *mut LastInput) -> i32; }
    #[link(name = "kernel32")]
    extern "system" { fn GetTickCount() -> u32; }
    let mut info = LastInput { size: std::mem::size_of::<LastInput>() as u32, tick: 0 };
    // Both clocks use the same wrapping millisecond counter. A failed query
    // counts as active, so an update cannot interrupt input we couldn't read.
    if unsafe { GetLastInputInfo(&mut info) } == 0 { return 0.; }
    unsafe { GetTickCount() }.wrapping_sub(info.tick) as f64 / 1000.
}

/// On Linux there's no asking yet, so it never counts as idle: in front it
/// waits for Restart to Update or for another app to be in front.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn idle_seconds() -> f64 {
    0.
}

impl Threadbear {
    /// From the app's start: only the installed app follows main.
    pub(crate) fn start_updater(&mut self, cx: &mut Context<Self>) {
        // The incoming side of a handover starts it once it takes over: the
        // app that's showing is the one following main.
        if crate::handover::waiting() {
            return;
        }
        let info = match followable() {
            Ok(info) => info,
            Err(why) => {
                // Only the installed app says why: every other launch is meant not to.
                if installed::launch().installed {
                    note(&format!("not following main: {why}"));
                }
                self.updater.disabled = Some(why);
                return;
            }
        };
        self.updater.staged = staged_build();
        note(&format!("running {} from {}; following main", info.short, info.checkout));
        let checkout = info.checkout.clone();
        self.updater.running = Some(info);
        let looks = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_LOOK).await;
            loop {
                if this.update(cx, |t, cx| t.check_for_update(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(LOOK_EVERY).await;
            }
        });
        let restarts = self.restart_looks(cx);
        self.updater.tasks = vec![looks, restarts];
        // A merge to main is noticed within seconds, not at the next look: its
        // ref is written in refs/heads, or packed into the git folder.
        let git = Path::new(&checkout).join(".git");
        if git.is_dir() {
            let mut watch = crate::file_watch::FolderWatch::new(cx, |this: &mut Threadbear, _, _, cx| this.check_for_update_soon(cx));
            if let Some(w) = watch.as_mut() {
                w.set(&[git.join("refs").join("heads"), git].iter().map(|p| p.to_string_lossy().into_owned()).collect());
            }
            self.updater.watch = watch;
        }
    }

    /// The once-a-second look at whether to restart into a staged build.
    fn restart_looks(&self, cx: &mut Context<Self>) -> Task<()> {
        let main = self.main_window;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(RESTART_LOOK).await;
            // The windows are looked at here, outside the app's own update:
            // a window's dialogs and focus are read through its root, which
            // reads the app back.
            let windows = cx.update(|cx| windows_now(main, cx));
            if this.update(cx, |t, cx| t.consider_auto_restart(windows, cx)).is_err() {
                break;
            }
        })
    }

    /// Main moved: a look in a few seconds. A burst of ref writes (a merge,
    /// then packed-refs) makes one look.
    fn check_for_update_soon(&mut self, cx: &mut Context<Self>) {
        self.updater.soon = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            this.update(cx, |t, cx| t.check_for_update(cx)).ok();
        }));
    }

    /// Reads main's desktop/ tree, including during a build, and waits for
    /// nearby landings to settle before requesting another.
    pub(crate) fn check_for_update(&mut self, cx: &mut Context<Self>) {
        let Some(running) = self.updater.running.clone() else { return };
        if self.updater.preview || self.updater.restarting {
            return;
        }
        // followMain: false in runner.json stops the runner rebuilding, and
        // the app too (on-your-mac.md promises both). Only an explicit false.
        if self.runner_settings.as_ref().and_then(|s| s["followMain"].as_bool()) == Some(false) {
            self.updater.settling = false;
            return;
        }
        let checkout = running.checkout.clone();
        cx.spawn(async move |this, cx| {
            let tree = cx.background_executor().spawn(async move { main_tree(&checkout) }).await;
            this.update(cx, |t, cx| {
                let Some(tree) = tree else { return };
                let ready = t.updater.quiet_main.ready(&tree, std::time::Instant::now());
                // followMain can have changed while git was being read.
                if t.runner_settings.as_ref().and_then(|s| s["followMain"].as_bool()) == Some(false) {
                    t.updater.settling = false;
                    return;
                }
                // What's staged may have been swapped in or dropped by hand.
                t.updater.staged = staged_build();
                t.updater.settling = false;
                match rules::decide(&tree, &running, t.updater.staged.as_ref(), t.updater.building.as_deref(), t.updater.failed.as_deref(), t.updater.restarting) {
                    Action::Nothing => {}
                    Action::DropStaged => {
                        let _ = std::fs::remove_dir_all(next_dir());
                        t.updater.staged = None;
                        note("dropped the staged build; main is what's running again");
                    }
                    Action::Build(tree) => {
                        if ready {
                            t.start_update_build(tree, running.checkout.clone(), cx);
                        } else {
                            t.updater.settling = true;
                            t.updater.waiting_for_host = false;
                        }
                    }
                }
                // The app menu's Restart to Update is there only while one is staged.
                t.menus_stale = true;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_update_build(&mut self, tree: String, checkout: String, cx: &mut Context<Self>) {
        if self.updater.building.is_some() || self.updater.restarting {
            return;
        }
        let Some(script) = script() else { return };
        let (program, arguments) = rules::build_command(&script.to_string_lossy(), &checkout, &next_dir().to_string_lossy());
        let log = open_log();
        let (out, err) = match log.as_ref().and_then(|l| Some((l.try_clone().ok()?, l.try_clone().ok()?))) {
            Some((a, b)) => (Stdio::from(a), Stdio::from(b)),
            None => (Stdio::null(), Stdio::null()),
        };
        let child = Command::new(program).args(arguments).current_dir(installed::home_dir()).stdin(Stdio::null()).stdout(out).stderr(err).spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                note(&format!("couldn't start the update build: {e}"));
                return;
            }
        };
        note(&format!("building main (desktop/ {})", &tree[..tree.len().min(12)]));
        self.updater.building = Some(tree.clone());
        self.updater.build_pid = Some(child.id());
        self.updater.waiting_for_host = false;
        cx.spawn(async move |this, cx| {
            let status = cx.background_executor().spawn(async move { child.wait().ok().and_then(|s| s.code()) }).await;
            this.update(cx, |t, cx| t.update_build_finished(tree, status, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn update_build_finished(&mut self, tree: String, status: Option<i32>, cx: &mut Context<Self>) {
        self.updater.building = None;
        self.updater.build_pid = None;
        let no_builder = status == Some(5);
        if no_builder && !self.updater.no_builder {
            note("no Mac is trusted to build the update: add one to updateBuildHosts in runner.json (check hosts only run tests)");
        }
        self.updater.no_builder = no_builder;
        match status {
            Some(0) => match staged_build() {
                Some(info) => {
                    note(&format!("{} is staged; it goes in when the app quits or is opened again", info.short));
                    self.updater.staged = Some(info);
                    self.updater.failed = None;
                    // The next of the once-a-second looks weighs restarting
                    // into it, with what the windows have up.
                    self.menus_stale = true;
                }
                None => self.updater.failed = Some(tree),
            },
            // No Mac answered, or its lease was busy: the next look tries again.
            Some(4) => self.updater.waiting_for_host = true,
            // None is trusted to: also tried again, once Mike names one.
            Some(5) => {}
            // Another build holds the lock: the next look sees how it went.
            Some(3) => {}
            _ => {
                // Not tried again until main moves on or the app is opened again.
                note(&format!("the update build failed ({status:?}); not tried again until main moves on"));
                self.updater.failed = Some(tree);
            }
        }
        // Main may have moved during the build. Observe it now, considering
        // only that latest tree, rather than replaying each landing.
        if status == Some(0) {
            self.check_for_update(cx);
        }
        cx.notify();
    }

    /// Words or pictures in a message box (one still being copied in too), a
    /// schedule editor open, or words in any other field that aren't saved
    /// anywhere yet (unsent.rs: a card's Something else, a project's goals
    /// being edited, a new title, a secret): the Swift app doesn't restart
    /// under the first two (`hasDrafts`), as Mike may be halfway through.
    fn has_unsent_words(&self, cx: &App) -> bool {
        let boxes = self.composers.values().chain(self.new_thread.composer.iter());
        boxes.into_iter().any(|c| c.read(cx).holds_anything(cx)) || self.schedule_editor.is_some() || crate::unsent::any(cx)
    }

    /// Something of the app's own under way that a restart would cut off: a
    /// drag, a page or row swipe, the button held (a selection being made),
    /// one of its menus, the share popover or the Move to sheet, a shell
    /// that's busy or typed in (quitting ends them all), or a web panel's
    /// page in use (Codex's review, 12 Nov).
    fn under_way(&self, cx: &App) -> bool {
        crate::terminal::holds_a_restart(cx)
            || crate::web_panels::holds_a_restart(cx)
            || cx.has_active_drag()
            || crate::reply_links::left_held()
            || self.swipe_nav.under_way()
            || self.sidebar.gestures.under_way()
            || self.sidebar.menu.is_some()
            || crate::tips::any_menu_up()
            || gpui_kit::base::GlobalState::is_in_deferred_context(cx)
            || self.share_popover.is_some()
            || self.anything.is_some()
            || self.move_sheet.is_some()
    }

    /// What holds a restart, as the end of "It switches in once ...", for
    /// the Updates panel and the log: the first of `has_unsent_words`,
    /// `under_way` and the windows' that holds, with the thread whose
    /// message box has words in it named, as that's the one he can't see.
    fn restart_hold(&self, windows: WindowsNow, cx: &App) -> String {
        let s = |text: &str| text.to_owned();
        if let Some((id, _)) = self.composers.iter().find(|(_, c)| c.read(cx).holds_anything(cx)) {
            return format!("the message box in “{}” is sent or cleared", self.thread_title(id));
        }
        if self.new_thread.composer.as_ref().is_some_and(|c| c.read(cx).holds_anything(cx)) {
            return s("the new thread's message box is sent or cleared");
        }
        if self.schedule_editor.is_some() {
            return s("the schedule you're editing is saved or closed");
        }
        if let Some(field) = crate::unsent::first(cx) {
            return format!("what you're typing in {field} is sent, saved or cleared");
        }
        if crate::terminal::holds_a_restart(cx) {
            return s("the Terminal's shell is back at an empty prompt");
        }
        if crate::web_panels::holds_a_restart(cx) {
            return s("a panel's page has been left alone for a minute");
        }
        if cx.has_active_drag() || crate::reply_links::left_held() || self.swipe_nav.under_way() || self.sidebar.gestures.under_way() {
            return s("the drag or swipe ends");
        }
        if windows.other_window_input {
            return s("you've finished typing in the other window");
        }
        s("the open menu, popover or dialog is closed")
    }

    /// A hold that's lasted a minute goes in the update log, once.
    fn note_long_hold(&mut self, wait: Option<&rules::RestartWait>) {
        let Some(rules::RestartWait::Held(why)) = wait else {
            self.updater.held_since = None;
            return;
        };
        match &mut self.updater.held_since {
            Some((was, since, noted)) if was == why => {
                if !*noted && since.elapsed() >= Duration::from_secs(60) {
                    *noted = true;
                    note(&format!("the staged build waits until {why}"));
                }
            }
            _ => self.updater.held_since = Some((why.clone(), std::time::Instant::now(), false)),
        }
    }

    /// What a restart by itself is weighed against just now, with what the
    /// windows have up (`windows_now`). A script reads it too (`updates` with
    /// `restartCheck`), as a scripted app never restarts.
    pub(crate) fn restart_moment(&self, windows: WindowsNow, cx: &App) -> rules::RestartMoment {
        rules::RestartMoment {
            staged: self.updater.staged.is_some(),
            loaded: self.sidebar.loaded && matches!(self.status, threadbear_client::runner::Status::Connected { .. }),
            has_drafts: self.has_unsent_words(cx),
            held: windows.held || self.under_way(cx),
            other_window_input: windows.other_window_input,
            active: app_active(),
            idle_for: idle_seconds(),
        }
    }

    /// Restarts into the staged build as soon as that wouldn't interrupt
    /// anything (`should_auto_restart`): checked every second, with what the
    /// windows have up (`windows_now`).
    pub(crate) fn consider_auto_restart(&mut self, windows: WindowsNow, cx: &mut Context<Self>) {
        // Looked at every second, staged or not, so a page used just before
        // a build is staged still counts as used lately.
        crate::web_panels::holds_a_restart(cx);
        // A scripted app's made-up update (`preview`) says what it would
        // wait on, for pictures and specs, but never restarts.
        if self.updater.restarting || self.updater.staged.is_none() {
            self.updater.held_since = None;
            if self.updater.wait.take().is_some() {
                cx.notify();
            }
            return;
        }
        let moment = self.restart_moment(windows, cx);
        let wait = rules::restart_wait(&moment, || self.restart_hold(windows, cx));
        self.note_long_hold(wait.as_ref());
        // Drawn again only when the line changes: once a second while it
        // counts down, not at all while it's held or he's using the app.
        if wait != self.updater.wait {
            self.updater.wait = wait;
            cx.notify();
        }
        if !self.updater.preview && rules::should_auto_restart(&moment) {
            note(if moment.active { "restarting into the staged build: nothing typed, and no input for five seconds" } else { "restarting into the staged build: nothing typed, and the app isn't in front" });
            // In front it comes back in front; behind, it stays behind.
            self.updater.auto_restart = true;
            self.restart_for_update(!moment.active, cx);
        }
    }

    /// Restart to Update: quit (which swaps the staged bundle in) and open the
    /// app again. Only the installed app: a scripted one showing a made-up
    /// update would otherwise quit and open Mike's.
    pub(crate) fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        if self.updater.staged.is_some() && self.updater.running.is_some() && !self.updater.preview {
            self.restart_for_update(false, cx);
        }
    }

    fn restart_for_update(&mut self, in_background: bool, cx: &mut Context<Self>) {
        self.updater.restarting = true;
        // Where it was, put back once the threads have loaded: the list,
        // the thread, where each thread was scrolled to, and whether it was
        // in front, so a restart made behind another app comes back behind it.
        self.settings.saved.update_restore = Some(self.restore_state(!in_background, cx));
        self.settings.save();
        // Kept in memory too: the old way's quit saves the settings again.
        // A handover drops it as it hands over (`leave_state_for_incoming`).
        // Next tick, outside whatever's being drawn or dispatched, so the
        // window can be asked where it is.
        cx.spawn(async move |this, cx| {
            this.update(cx, |t, cx| t.hand_over_or_relaunch(in_background, cx)).ok();
        })
        .detach();
    }

    /// With the window plainly on screen, the new build is swapped in now
    /// and started beside this one, which quits once it's showing
    /// (handover.rs), so the window never goes away. Otherwise, or if that
    /// can't start, the old way: quit, and open the app again.
    fn hand_over_or_relaunch(&mut self, in_background: bool, cx: &mut Context<Self>) {
        let main = self.main_window;
        let spot = main.update(cx, |_, window, _| crate::handover::outgoing_spot(window)).ok().flatten();
        if let Some(spot) = spot {
            if drop_untrusted_staged() {
                match rules::apply_staged(&install_dir(), &next_dir(), &old_dir(), &swapping_note(), self.updater.running.as_ref()) {
                    Ok(Some(staged)) => {
                        note(&format!("swapped in {} to hand over to", staged.short));
                        self.updater.staged = None;
                        self.hand_over(install_dir(), spot, move |_, cx| {
                            relaunch_after_exit(&install_dir(), in_background);
                            cx.quit();
                        }, cx);
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => note(&e),
                }
            }
        }
        relaunch_after_exit(&install_dir(), in_background);
        cx.quit();
    }

    /// Where each thread open in a pane is scrolled to now, the ones at
    /// their end too.
    pub(crate) fn live_scroll_memories(&self, cx: &App) -> Vec<(String, crate::rows::ScrollMemory)> {
        self.threads
            .keys()
            .filter_map(|id| {
                let pane = self.pane_for(id, cx)?;
                let pane = pane.read(cx);
                pane.is_loaded().then(|| (id.clone(), pane.capture_memory(cx)))
            })
            .collect()
    }

    /// Something a restart by itself would have waited for, started while a
    /// handover's new copy loaded: it holds the go (handover.rs). Not words
    /// in a message box, which go across in the drafts file, but a picture
    /// still being copied into one, which doesn't; and nothing for a Restart
    /// to Update he chose, which never waited for these.
    pub(crate) fn handover_held(&self, windows: WindowsNow, cx: &App) -> bool {
        let copying = self.composers.values().chain(self.new_thread.composer.iter()).any(|c| c.read(cx).copying_picture(cx));
        self.updater.auto_restart && (windows.held || windows.other_window_input || copying || self.under_way(cx) || self.schedule_editor.is_some() || crate::unsent::any(cx))
    }

    /// What a restart leaves for the next launch: the list and the thread
    /// showing, whether it was in front, and where each thread was scrolled
    /// to (the open ones measured now; ones opened earlier as they were
    /// left), so none of them jumps to its end.
    pub(crate) fn restore_state(&self, front: bool, cx: &App) -> serde_json::Value {
        let live = self.live_scroll_memories(cx);
        json!({ "mode": self.mode.name(), "selection": self.selected, "front": front, "scroll": crate::rows::scroll_memories_json(&live) })
    }

    /// On quit: the staged bundle takes the installed one's place, so the
    /// next launch is the new app. The running one goes to `.old` until
    /// then. A build still going is stopped (only ever our own child).
    pub(crate) fn apply_update_at_quit(&mut self) {
        // Only the installed app, following main for real: a scripted or
        // debug app showing a made-up update (`preview_update`) must never
        // swap or delete what's at the install place (Codex's review, 12 Nov).
        if self.updater.running.is_none() || self.updater.preview || !installed::launch().installed {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.updater.build_pid.take() {
            // SAFETY: a signal to the build we started, still ours to wait on.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
        if !drop_untrusted_staged() {
            return;
        }
        match rules::apply_staged(&install_dir(), &next_dir(), &old_dir(), &swapping_note(), self.updater.running.as_ref()) {
            Ok(Some(staged)) => note(&format!("swapped in {} at quit", staged.short)),
            Ok(None) => {}
            Err(e) => note(&e),
        }
    }

    /// After a restart the app made itself: back to the list and the thread
    /// that were showing, the thread once the list is in (`pending_restore`),
    /// without bringing the window in front.
    pub(crate) fn restore_after_update(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self.settings.saved.update_restore.take() else { return };
        self.save_settings_soon(cx);
        // Where each thread was scrolled to, before any of them opens: the
        // pane looks there as it loads (`restore_memory`).
        crate::rows::recall_scroll_memories(&saved["scroll"]);
        self.restore_place(saved["mode"].as_str().unwrap_or("inbox"), saved["selection"].as_str().unwrap_or(""), cx);
    }

    /// Puts the list and the thread back. Only what differs is changed, so a
    /// handover's "go" that matches what's showing does nothing.
    pub(crate) fn restore_place(&mut self, mode: &str, selection: &str, cx: &mut Context<Self>) {
        let mode = match mode {
            "scheduled" => crate::shell::SidebarMode::Scheduled,
            "projects" => crate::shell::SidebarMode::Projects,
            "librarian" => crate::shell::SidebarMode::Librarian,
            _ => crate::shell::SidebarMode::Inbox,
        };
        if self.mode != mode {
            self.set_mode(mode, cx);
        }
        if !selection.is_empty() && self.selected.as_deref() != Some(selection) {
            self.pending_restore = Some(selection.to_owned());
            // The list is in already (a script's restart, or a handover's
            // "go"): open it now.
            if self.sidebar.loaded {
                self.open_pending(cx);
            }
        }
    }

    /// The app's side for the Updates panel and its icon (`UpdatesPanel.appSide`).
    pub(crate) fn app_update_side(&self) -> threadbear_core::updates::AppSide {
        let u = &self.updater;
        threadbear_core::updates::AppSide {
            running: u.running.as_ref().map(|b| b.short.clone()),
            staged: u.staged.as_ref().map(|b| b.short.clone()),
            building: u.building.is_some(),
            settling: u.settling && self.runner_settings.as_ref().and_then(|s| s["followMain"].as_bool()) != Some(false),
            failed: u.failed.is_some() && u.staged.is_none(),
            waiting: u.waiting_for_host && u.staged.is_none() && u.building.is_none(),
            // Only the installed app's reasons: every other launch is meant
            // not to. No builder is only ever set by an installed app's build
            // (or a scripted app's made-up update).
            not_following: u.disabled.clone().filter(|_| installed::launch().installed).or_else(|| u.no_builder.then(|| NO_BUILDER.to_owned())),
            wait: u.wait.clone().filter(|_| u.staged.is_some()),
        }
    }

    /// A made-up update that's "staged", "building", "settling", "failed", "waiting" or "no-builder"
    /// (or none, on abc1234), for a scripted app's pictures and specs, as
    /// the Swift app's `previewUpdate`. Only scripts reach it, through the
    /// Updates panel's `updates` command.
    pub(crate) fn preview_update(&mut self, state: &str, cx: &mut Context<Self>) {
        let build = |short: &str| BuildInfo { commit: short.into(), short: short.into(), app_tree: short.into(), configuration: "debug".into(), ..Default::default() };
        let u = &mut self.updater;
        u.preview = true;
        u.running = Some(build("abc1234"));
        u.staged = (state == "staged").then(|| build("def5678"));
        u.building = (state == "building").then(|| "def5678".to_owned());
        u.settling = state == "settling";
        u.failed = (state == "failed").then(|| "def5678".to_owned());
        u.waiting_for_host = state == "waiting";
        u.no_builder = state == "no-builder";
        // A scripted app never follows main, so its looks start here: what
        // a made-up staged build waits on is said as the real one's is.
        if u.tasks.is_empty() {
            let restarts = self.restart_looks(cx);
            self.updater.tasks.push(restarts);
        }
        self.menus_stale = true;
        cx.notify();
    }
}
