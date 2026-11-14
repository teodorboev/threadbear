// Restarting onto an update without the window going away: a port of the
// Swift app's Handover.swift (core's handover.rs has the plan and the pure
// parts). Mac only: elsewhere a restart is still the window going and coming
// back (app_update.rs, `relaunch_after_exit`).
//
// The outgoing side runs from `restart_for_update` once the staged build is
// in place: it starts that build as a second copy (`open -n`), listens for
// "ready", hands over the keys if it had them, says "go", and quits once the
// new copy says "done" (or a second later). The incoming side runs from
// `main` and `Threadbear::new`: its window opens unseen (alpha 0, below the
// outgoing one, taking no clicks), it loads, then shows itself under the old
// window and says "ready"; at "go" it puts back anything that changed, takes
// the drafts, and becomes the app. If the outgoing app goes without a "go"
// it takes over anyway; if the incoming app never says ready, the outgoing
// one quits for it, or restarts the old way if it never started.
use crate::app::Threadbear;
use gpui_kit::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use threadbear_core::handover::{self as rules, Go, Incoming};

/// This launch's handover, if it's the incoming side.
fn incoming() -> Option<&'static Incoming> {
    static INCOMING: OnceLock<Option<Incoming>> = OnceLock::new();
    INCOMING.get_or_init(|| rules::incoming(&std::env::args().collect::<Vec<_>>())).as_ref()
}

/// Taken over: from then on it's the app, as any launch is.
static TAKEN_OVER: AtomicBool = AtomicBool::new(false);
/// The outgoing app said "go": what it would write as it quits (drafts,
/// settings) is the incoming app's now.
static HANDED_OVER: AtomicBool = AtomicBool::new(false);

/// The incoming app, waiting to take over: its window is unseen, and it
/// leaves notifications and following main to the app that's showing.
pub(crate) fn waiting() -> bool {
    incoming().is_some() && !TAKEN_OVER.load(Ordering::Acquire)
}

/// The outgoing app has handed over: its quit writes nothing.
pub(crate) fn handed_over() -> bool {
    HANDED_OVER.load(Ordering::Acquire)
}

fn channel(pid: u32) -> PathBuf {
    rules::channel_dir(&std::env::temp_dir(), pid)
}

fn log(line: &str) {
    threadbear_client::log::write("handover", line);
}

/// Is a process still there.
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only asks whether the pid exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    false
}

/// First thing in `main` for the incoming app: says it started (with its
/// pid, so the outgoing app can tell a slow launch from a failed one).
pub(crate) fn announce_start() {
    if let Some(from) = incoming() {
        let dir = channel(from.from);
        let _ = rules::write_message(&dir, "started.json", &rules::ready_message(std::process::id()));
        log(&format!("incoming from {}", from.from));
    }
}

/// The incoming app's window: never shown or focused by its opening.
pub(crate) fn window_options_hidden() -> bool {
    incoming().is_some()
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSRunningApplication, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use threadbear_core::handover::Frame;

    pub(super) fn ns_window(window: &gpui_kit::Window) -> Option<Retained<NSWindow>> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
        // SAFETY: the window's own NSView, alive while the window is.
        let view: &objc2_app_kit::NSView = unsafe { &*h.ns_view.as_ptr().cast() };
        view.window()
    }

    /// Its number and frame, if it's plainly on screen (`windowToHandOver`).
    pub(super) fn spot(window: &NSWindow) -> Option<(i64, Frame)> {
        let mtm = MainThreadMarker::new()?;
        let hidden = NSApplication::sharedApplication(mtm).isHidden();
        let ok = threadbear_core::handover::can_hand_over(window.isVisible(), window.isMiniaturized(), window.styleMask().contains(NSWindowStyleMask::FullScreen), hidden);
        let f = window.frame();
        ok.then(|| (window.windowNumber() as i64, Frame { x: f.origin.x, y: f.origin.y, width: f.size.width, height: f.size.height }))
    }

    /// Unseen, taking no clicks, in the outgoing window's place and just
    /// under it, so it lays out and draws as if it were showing.
    pub(super) fn conceal(window: &NSWindow, frame: Option<Frame>, below: Option<i64>) {
        window.setAlphaValue(0.);
        window.setIgnoresMouseEvents(true);
        // NSWindowAnimationBehaviorNone: no zoom in or out as it shows or goes.
        let _: () = unsafe { msg_send![window, setAnimationBehavior: 2isize] };
        if let Some(f) = frame {
            let rect = NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.width, f.height));
            window.setFrame_display(rect, false);
        }
        order_below(window, below);
    }

    fn order_below(window: &NSWindow, below: Option<i64>) {
        match below {
            // NSWindowBelow (-1) relative to the outgoing app's window.
            Some(n) => {
                let _: () = unsafe { msg_send![window, orderWindow: -1isize, relativeTo: n as isize] };
            }
            None => window.orderBack(None),
        }
    }

    /// Visible, just under the outgoing app's window: nothing to see yet, as
    /// the two are in the same place showing the same thing.
    pub(super) fn show_under(window: &NSWindow, below: Option<i64>) {
        order_below(window, below);
        window.setAlphaValue(1.);
    }

    pub(super) fn take_over(window: &NSWindow, activate: bool) {
        window.setAlphaValue(1.);
        window.setIgnoresMouseEvents(false);
        // Taking over before it was shown under the old one (the old app
        // went early): it isn't on screen at all yet.
        if !window.isVisible() {
            window.orderFront(None);
        }
        // Back to the default (NSWindowAnimationBehaviorDefault).
        let _: () = unsafe { msg_send![window, setAnimationBehavior: 0isize] };
        if activate {
            if let Some(mtm) = MainThreadMarker::new() {
                #[allow(deprecated)]
                NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            }
            window.makeKeyAndOrderFront(None);
        }
    }

    pub(super) fn app_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
    }

    /// The keys go to the incoming app, as the outgoing one quits: it was
    /// in front, so the new one is (yieldActivation, macOS 14 and on; it
    /// activates itself at "go" too).
    pub(super) fn yield_to(pid: u32) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) else { return };
        let app = NSApplication::sharedApplication(mtm);
        let can: bool = unsafe { msg_send![&*app, respondsToSelector: objc2::sel!(yieldActivationToApplication:)] };
        if can {
            let _: () = unsafe { msg_send![&*app, yieldActivationToApplication: &*other] };
        }
    }

    /// The old window's last moment: clicks pass to the new one under it.
    pub(super) fn stop_taking_clicks() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        for window in NSApplication::sharedApplication(mtm).windows().iter() {
            window.setIgnoresMouseEvents(true);
        }
    }

    /// Every window of this app off the screen, without the closing
    /// animation, so the incoming one under them is what shows.
    pub(super) fn hide_all() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        for window in NSApplication::sharedApplication(mtm).windows().iter() {
            if window.isVisible() {
                let _: () = unsafe { msg_send![&*window, setAnimationBehavior: 2isize] };
                window.orderOut(None);
            }
        }
    }
}

/// Opened: the incoming app's window, unseen until it's ready.
pub(crate) fn conceal(window: &Window) {
    #[cfg(target_os = "macos")]
    if let (Some(from), Some(ns)) = (incoming(), mac::ns_window(window)) {
        mac::conceal(&ns, from.frame, from.below);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// What a handover needs of the outgoing window: its number and frame, if it
/// can be handed over. Never a scripted app's (it never restarts itself).
pub(crate) fn outgoing_spot(window: &Window) -> Option<(i64, rules::Frame)> {
    if crate::scripted::scripted() {
        return None;
    }
    #[cfg(target_os = "macos")]
    return mac::spot(&*mac::ns_window(window)?);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// The environment `open` is to pass on (app_update.rs has why).
fn open_command(bundle: &Path, arguments: &[String]) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.args(["-n", "-g"]);
    for (key, value) in crate::app_update::settings_to_pass_on() {
        let mut pair = std::ffi::OsString::from(format!("{key}="));
        pair.push(value);
        command.arg("--env").arg(pair);
    }
    command.arg(bundle).arg("--args").args(arguments);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    command
}

impl Threadbear {
    /// The outgoing side: `bundle` (the new build, swapped in already) as a
    /// second copy, this one quitting once it's showing. `fallback` is the
    /// old way, for when it can't start.
    pub(crate) fn hand_over(&mut self, bundle: PathBuf, spot: (i64, rules::Frame), fallback: impl FnOnce(&mut Threadbear, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let pid = std::process::id();
        let dir = channel(pid);
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log(&format!("couldn't make {}: {e}; restarting the old way", dir.display()));
            return fallback(self, cx);
        }
        let current: Vec<String> = std::env::args().skip(1).collect();
        let arguments = rules::arguments(&current, pid, spot.0, spot.1);
        log(&format!("starting {} to hand over to", bundle.display()));
        let started = open_command(&bundle, &arguments).status();
        if !started.as_ref().is_ok_and(|s| s.success()) {
            log(&format!("couldn't start the new app ({started:?}); restarting the old way"));
            let _ = std::fs::remove_dir_all(&dir);
            return fallback(self, cx);
        }
        let mut fallback = Some(fallback);
        let main = self.main_window;
        cx.spawn(async move |this, cx| {
            let began = Instant::now();
            loop {
                cx.background_executor().timer(rules::POLL).await;
                // The old window takes clicks and keys while the new copy
                // loads: anything the restart would have waited for that
                // started meanwhile (a shell, a menu, a field being typed
                // in) holds the go, as it held the restart (Codex's review).
                let windows = cx.update(|cx| crate::app_update::windows_now(main, cx));
                if this.update(cx, |t, cx| t.handover_held(windows, cx)).unwrap_or(false) {
                    continue;
                }
                if let Some(ready) = rules::read_message(&dir, rules::READY_FILE) {
                    let incoming_pid = rules::ready_pid(&ready);
                    // Ready, then gone (while a hold kept the go back, say):
                    // nothing to hand over to, so the old way (Codex's second look).
                    if !incoming_pid.is_some_and(alive) {
                        log("the new app said ready, then went; restarting the old way");
                        let _ = std::fs::remove_dir_all(&dir);
                        if let Some(f) = fallback.take() {
                            this.update(cx, |t, cx| f(t, cx)).ok();
                        }
                        break;
                    }
                    this.update(cx, |t, cx| t.say_go(incoming_pid, &dir, cx)).ok();
                    break;
                }
                if began.elapsed() >= rules::READY_TIMEOUT {
                    let started = rules::read_message(&dir, "started.json").as_ref().and_then(rules::ready_pid);
                    if started.is_some_and(alive) {
                        // It takes over when it sees this app go.
                        log("no ready after 20 s; quitting for it");
                        // Not without the restart's snapshot: it may not
                        // have read it yet (Codex's second look).
                        this.update(cx, |t, cx| {
                            t.leave_state_for_incoming(false, cx);
                            t.quit_handed_over(&dir, cx)
                        })
                        .ok();
                    } else {
                        log("the new app went away; restarting the old way");
                        let _ = std::fs::remove_dir_all(&dir);
                        if let Some(f) = fallback.take() {
                            this.update(cx, |t, cx| f(t, cx)).ok();
                        }
                    }
                    break;
                }
            }
        })
        .detach();
    }

    /// "ready": the keys handed over if this app had them, the drafts on disk
    /// for the incoming app, then "go" with where the window is now.
    fn say_go(&mut self, incoming_pid: Option<u32>, dir: &Path, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        let active = mac::app_active();
        #[cfg(not(target_os = "macos"))]
        let active = false;
        #[cfg(target_os = "macos")]
        if let Some(pid) = incoming_pid.filter(|_| active) {
            mac::yield_to(pid);
        }
        let _ = incoming_pid;
        // No more clicks in the old window from here: what it holds now is
        // what goes across (Codex's second look). The keys went with the
        // activation, and with another app in front it had none.
        #[cfg(target_os = "macos")]
        mac::stop_taking_clicks();
        // It read the restart's snapshot at launch, before it said ready.
        self.leave_state_for_incoming(true, cx);
        let selection = self.selected.clone().unwrap_or_default();
        // Where each open thread is scrolled to now, as he may have scrolled
        // while the new copy loaded (Codex's review, 13 Nov).
        let scroll = crate::rows::live_scroll_json(&self.live_scroll_memories(cx));
        let go = Go { active, mode: Some(self.mode.name().to_owned()), selection: Some(selection), scroll };
        if let Err(e) = rules::write_message(dir, rules::GO_FILE, &go.to_json()) {
            log(&format!("couldn't say go: {e}"));
        }
        let dir = dir.to_path_buf();
        cx.spawn(async move |this, cx| {
            let began = Instant::now();
            while !dir.join(rules::DONE_FILE).exists() && began.elapsed() < rules::DONE_TIMEOUT {
                cx.background_executor().timer(rules::POLL).await;
            }
            // No "done" and the new app gone: open it again once this one
            // has quit, as the old way would, rather than leave nothing.
            if !dir.join(rules::DONE_FILE).exists() && !incoming_pid.is_some_and(alive) {
                log("the new app went before taking over; opening it again");
                crate::app_update::relaunch_installed();
            }
            this.update(cx, |t, cx| t.quit_handed_over(&dir, cx)).ok();
        })
        .detach();
    }

    /// As at a quit, before this app stops writing anything: messages held
    /// for a restarting runner stay held, what's typed goes to the drafts file
    /// the incoming app reads as it takes over, and the settings are saved
    /// for it to read then too, without the restart's snapshot (it took that
    /// at launch, and a later launch mustn't restore it again).
    fn leave_state_for_incoming(&mut self, snapshot_taken: bool, cx: &mut Context<Self>) {
        if HANDED_OVER.load(Ordering::Acquire) {
            return;
        }
        self.client.stop_holding();
        self.save_drafts(cx);
        if snapshot_taken {
            self.settings.saved.update_restore = None;
        }
        self.settings_dirty = false;
        self.settings.save();
        HANDED_OVER.store(true, Ordering::Release);
    }

    fn quit_handed_over(&mut self, dir: &Path, cx: &mut Context<Self>) {
        log("handed over; quitting");
        let _ = std::fs::remove_dir_all(dir);
        #[cfg(target_os = "macos")]
        mac::hide_all();
        cx.quit();
    }

    /// The incoming side, from `Threadbear::new`: shows itself under the old
    /// window once it's loaded, then waits for "go" (or for the old app to go).
    pub(crate) fn start_incoming(&mut self, cx: &mut Context<Self>) {
        let Some(from) = incoming().cloned() else { return };
        let dir = channel(from.from);
        // Never taken over from here: the window's made, and concealed, only
        // after `Threadbear::new` returns (Codex's review, 13 Nov). The first
        // look, 50 ms on, sees an outgoing app that's already gone.
        cx.spawn(async move |this, cx| {
            let began = Instant::now();
            let mut ready = false;
            loop {
                cx.background_executor().timer(rules::POLL).await;
                if !waiting() {
                    break;
                }
                if !alive(from.from) {
                    this.update(cx, |t, cx| t.take_over(false, "the outgoing app is gone", &dir, cx)).ok();
                    break;
                }
                if !ready {
                    let Ok(loaded) = this.update(cx, |t, cx| rules::loaded(t.sidebar.loaded, t.connected(), t.selection_loaded(cx), began.elapsed())) else { break };
                    if loaded {
                        cx.background_executor().timer(rules::SETTLE_TIME).await;
                        this.update(cx, |t, cx| t.show_under_outgoing(&from, cx)).ok();
                        let _ = rules::write_message(&dir, rules::READY_FILE, &rules::ready_message(std::process::id()));
                        log(&format!("ready after {:.2} s", began.elapsed().as_secs_f64()));
                        ready = true;
                    }
                    continue;
                }
                if let Some(go) = rules::read_message(&dir, rules::GO_FILE) {
                    let go = Go::from_json(&go);
                    this.update(cx, |t, cx| t.go(go, &dir, cx)).ok();
                    break;
                }
            }
        })
        .detach();
    }

    fn connected(&self) -> bool {
        matches!(self.status, threadbear_client::runner::Status::Connected { .. })
    }

    /// The thread that was open has its first page (a project page, the
    /// schedules or nothing open count as loaded).
    fn selection_loaded(&self, cx: &App) -> bool {
        if self.pending_restore.is_some() {
            return false;
        }
        let Some(id) = self.selected.as_deref() else { return true };
        if !self.threads.contains_key(id) {
            return true;
        }
        // And its place, if that's in an older page still on its way.
        self.pane_for(id, cx).is_some_and(|p| p.read(cx).is_loaded() && !p.read(cx).restoring_place())
    }

    fn show_under_outgoing(&mut self, from: &Incoming, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if let Some(ns) = self.main_ns_window(cx) {
            mac::show_under(&ns, from.below);
        }
        let _ = (from, cx);
    }

    /// The main window's NSWindow. Asked from the once-every-50-ms look, never
    /// while the window is being drawn.
    #[cfg(target_os = "macos")]
    fn main_ns_window(&self, cx: &mut App) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
        let main = self.main_window;
        main.update(cx, |_, window, _| mac::ns_window(window)).ok().flatten()
    }

    /// "go": what Mike did in the outgoing window while this one loaded, the
    /// drafts it saved just before, then this app is the app.
    fn go(&mut self, go: Go, dir: &Path, cx: &mut Context<Self>) {
        // Where each open thread was scrolled to as it went: remembered for
        // threads not open here yet, and put straight onto ones that are.
        for (id, memory) in crate::rows::decode_live_scroll(&go.scroll) {
            crate::rows::remember_scroll(&id, memory.clone());
            if let Some(pane) = self.pane_for(&id, cx) {
                pane.update(cx, |p, cx| p.take_handed_memory(memory, cx));
            }
        }
        if let Some(mode) = go.mode.as_deref() {
            self.restore_place(mode, go.selection.as_deref().unwrap_or(""), cx);
        }
        self.take_over(go.active, "go", dir, cx);
        let _ = rules::write_message(dir, rules::DONE_FILE, &serde_json::json!({}));
    }

    fn take_over(&mut self, activate: bool, reason: &str, dir: &Path, cx: &mut Context<Self>) {
        if TAKEN_OVER.swap(true, Ordering::AcqRel) {
            return;
        }
        if reason != "go" {
            let _ = std::fs::remove_dir_all(dir);
        }
        // What the outgoing app saved as it handed over (or last saved, if
        // it went without a "go"): its settings, as this app's copy is from
        // before he may have changed any there, and the drafts, which this
        // app left alone until now (Codex's review, 13 Nov).
        let mut fresh = crate::settings::Settings::load();
        fresh.saved.update_restore = None;
        self.settings = fresh;
        self.reload_drafts(cx);
        #[cfg(target_os = "macos")]
        if let Some(ns) = self.main_ns_window(cx) {
            mac::take_over(&ns, activate);
        }
        log(&format!("took over ({reason}){}", if activate { ", in front" } else { "" }));
        // What only the app that's showing runs.
        self.start_updater(cx);
        cx.notify();
    }
}
