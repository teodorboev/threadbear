// What makes a scripted app (an `automation` build with THREADBEAR_AUTOMATION
// set: the specs, desktop/scripts/dev-app.sh) safe to run on the Mac someone's
// working on, as the Swift app's Automation has it (app/README.md): it won't
// start against the real data folder and its live runner, anything it copies
// goes to a pasteboard of its own rather than their clipboard, and with
// THREADBEAR_KEEP_BEHIND=1 its window stays behind their work, out of the Dock
// and the app switcher. Not covered: a program run in its Terminal panel
// (`pbcopy` there reaches the real clipboard, as from any shell). An agent's app found all three missing (the parity
// audit, "Agents can't safely drive GPUI on this Mac", 13 Nov).
use gpui_kit::*;

/// Driven by a script.
pub(crate) fn scripted() -> bool {
    cfg!(feature = "automation") && std::env::var_os("THREADBEAR_AUTOMATION").is_some()
}

/// dev-app.sh's instances: the window behind everything, never in the Dock.
pub(crate) fn keep_behind() -> bool {
    scripted() && std::env::var("THREADBEAR_KEEP_BEHIND").as_deref() == Ok("1")
}

/// First thing in `main`, before the platform reads the pasteboard: a
/// scripted app pointed at the real data folder says why and quits, and one
/// that isn't gets its own pasteboard.
pub(crate) fn guard() {
    if !scripted() {
        return;
    }
    use threadbear_client::config::{default_home, scripted_refusal, RunnerConfig};
    let allowed = std::env::var("THREADBEAR_AUTOMATION_LIVE").as_deref() == Ok("1");
    if let Some(why) = scripted_refusal(&RunnerConfig::from_env(), &default_home(), allowed) {
        eprintln!("{why}");
        std::process::exit(2);
    }
    // The web runtime is the machine's, in the real data folder, with the
    // panels' Chromium profile (Mike's sign-ins) and helper bundles the
    // installed app runs (threadbear_core::web_runtime). A scripted app
    // given none keeps one beside its automation file instead.
    if std::env::var_os("THREADBEAR_WEB_RUNTIME_DIR").is_none_or(|d| d.is_empty()) {
        if let Some(dir) = std::env::var_os("THREADBEAR_AUTOMATION").and_then(|f| std::path::Path::new(&f).parent().map(|p| p.join("web-runtime"))) {
            // Before any other thread starts: guard() is main's first line.
            std::env::set_var("THREADBEAR_WEB_RUNTIME_DIR", dir);
        }
    }
    // And neither that nor its automation file (beside which go its log
    // and crash reports) may be in the real data folder, however it's
    // named (Codex's second look).
    if !allowed {
        let resolved = |p: &std::path::Path| std::fs::canonicalize(p).ok().or_else(|| Some(std::fs::canonicalize(p.parent()?).ok()?.join(p.file_name()?))).unwrap_or_else(|| p.to_path_buf());
        let real = resolved(&default_home());
        for var in ["THREADBEAR_WEB_RUNTIME_DIR", "THREADBEAR_AUTOMATION"] {
            if let Some(path) = std::env::var_os(var) {
                if resolved(std::path::Path::new(&path)).starts_with(&real) {
                    eprintln!("A scripted app won't keep {var} in the real data folder ({}). Give it a folder of its own (desktop/scripts/dev-app.sh does).", real.display());
                    std::process::exit(2);
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    pasteboard::install();
}

/// The application, out of the Dock and the app switcher from launch when
/// it's kept behind: Cmd-Tabbing into one by mistake and typing into its
/// composer would start a real turn on a dev runner. On a Mac, GPUI's
/// "headless" is only that (the Accessory policy); its windows still open.
pub(crate) fn application(app: Application) -> Application {
    #[cfg(target_os = "macos")]
    if keep_behind() {
        return app.with_windowing(WindowingRequest::Headless);
    }
    app
}

/// The window behind every other app's, as soon as it's open.
pub(crate) fn send_behind(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        // SAFETY: the window's own NSView, alive while the window is.
        let view: &objc2_app_kit::NSView = unsafe { &*h.ns_view.as_ptr().cast() };
        if let Some(ns_window) = view.window() {
            ns_window.orderBack(None);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// The private pasteboard: `+[NSPasteboard generalPasteboard]` answers with
/// one of this app's own, so GPUI's clipboard (made from it as the platform
/// starts), the draft boxes' HTML copies and anything else asking for the
/// general one all use it, and the `clipboard` and `paste` commands read and
/// write it. Swapped in the whole process rather than at each copy, so a new
/// copy button can't forget. Linux's and Windows' scripted apps still use the
/// system's: the specs run there on machines nobody's working at.
#[cfg(target_os = "macos")]
pub(crate) mod pasteboard {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, Imp, Sel};
    use objc2::{sel, ClassType as _};
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;
    use std::sync::atomic::{AtomicPtr, Ordering};

    static BOARD: AtomicPtr<NSPasteboard> = AtomicPtr::new(std::ptr::null_mut());

    /// THREADBEAR_PASTEBOARD, else one named for this process.
    pub(crate) fn name() -> String {
        std::env::var("THREADBEAR_PASTEBOARD").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("com.mikerosoft.threadbear.automation.{}", std::process::id()))
    }

    // Returned unretained, as generalPasteboard's own is: BOARD keeps its
    // one reference for as long as the process runs.
    extern "C-unwind" fn general(_class: &AnyClass, _sel: Sel) -> *mut NSPasteboard {
        BOARD.load(Ordering::Acquire)
    }

    pub(crate) fn install() {
        let board = NSPasteboard::pasteboardWithName(&NSString::from_str(&name()));
        board.clearContents();
        BOARD.store(Retained::into_raw(board), Ordering::Release);
        let method = NSPasteboard::class().class_method(sel!(generalPasteboard)).expect("NSPasteboard has generalPasteboard");
        // SAFETY: the same signature as the method it replaces (a class
        // method taking nothing and returning an NSPasteboard), set once,
        // before anything else in the process has asked for it.
        unsafe { method.set_implementation(std::mem::transmute::<extern "C-unwind" fn(&AnyClass, Sel) -> *mut NSPasteboard, Imp>(general)) };
        // Given back to the system when the app quits, so a day of runs
        // doesn't leave hundreds of them in the pasteboard server.
        extern "C" fn release() {
            let board = BOARD.load(Ordering::Acquire);
            if !board.is_null() {
                // SAFETY: BOARD's own reference, still held; objc2-app-kit
                // doesn't bind releaseGlobally, so it's sent by name.
                let _: () = unsafe { objc2::msg_send![&*board, releaseGlobally] };
            }
        }
        // SAFETY: a plain function with no arguments, as atexit takes.
        unsafe { libc::atexit(release) };
    }

    #[cfg(test)]
    mod tests {
        // One test, as the swap is for the whole process.
        #[test]
        fn the_general_pasteboard_is_a_private_one() {
            std::env::set_var("THREADBEAR_PASTEBOARD", format!("com.mikerosoft.threadbear.test.{}", std::process::id()));
            super::install();
            let general = objc2_app_kit::NSPasteboard::generalPasteboard();
            assert_eq!(general.name().to_string(), super::name());
            general.clearContents();
            general.setString_forType(&objc2_foundation::NSString::from_str("private"), unsafe { objc2_app_kit::NSPasteboardTypeString });
            let again = objc2_app_kit::NSPasteboard::generalPasteboard();
            assert_eq!(again.stringForType(unsafe { objc2_app_kit::NSPasteboardTypeString }).map(|s| s.to_string()).as_deref(), Some("private"));
        }
    }
}
