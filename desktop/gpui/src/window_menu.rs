// The menu bar's Edit menu and the window commands in File and Window, as the
// Swift app gets them from AppKit and SwiftUI: Undo to Select All for the text
// boxes and the transcript's selection, File > Close (⌘W), and Window's
// Minimize, Zoom and Bring All to Front. AppKit adds the rest to a menu
// called Edit or Window by itself on a Mac (AutoFill, Start Dictation, Emoji
// & Symbols; Fill, Center, Move & Resize, and the list of open windows), and
// Enter Full Screen to View, as it does for the Swift app.
//
// Closing the main window quits the app, as the Swift app's
// `applicationShouldTerminateAfterLastWindowClosed` has it: GPUI on a Mac
// otherwise leaves the app running with no window and no way to get one back.
use gpui_kit::*;

/// An item of the Edit menu. Each is the text box's own action (gpui-kit's
/// input), so a menu item does what its key does wherever the keyboard is:
/// the composer, a field, or words selected in a reply (thread.rs takes
/// Copy there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditItem {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// Words only, as the Mac's own; the message box takes it
    /// (composer_spelling.rs), and it's greyed elsewhere.
    PasteAndMatchStyle,
    Delete,
    SelectAll,
}

impl EditItem {
    pub(crate) fn title(self) -> &'static str {
        match self {
            EditItem::Undo => "Undo",
            EditItem::Redo => "Redo",
            EditItem::Cut => "Cut",
            EditItem::Copy => "Copy",
            EditItem::Paste => "Paste",
            EditItem::PasteAndMatchStyle => "Paste and Match Style",
            EditItem::Delete => "Delete",
            EditItem::SelectAll => "Select All",
        }
    }

    pub(crate) fn action(self) -> Box<dyn Action> {
        use gpui_kit::component::input;
        match self {
            EditItem::Undo => Box::new(input::Undo),
            EditItem::Redo => Box::new(input::Redo),
            EditItem::Cut => Box::new(input::Cut),
            EditItem::Copy => Box::new(input::Copy),
            EditItem::Paste => Box::new(input::Paste),
            EditItem::PasteAndMatchStyle => Box::new(crate::composer_spelling::PasteAndMatchStyle),
            EditItem::Delete => Box::new(input::Delete),
            EditItem::SelectAll => Box::new(input::SelectAll),
        }
    }

    /// The system's own selector for it, so a save panel or another
    /// AppKit view in front takes the item as it would in any Mac app.
    pub(crate) fn os_action(self) -> Option<OsAction> {
        match self {
            EditItem::Undo => Some(OsAction::Undo),
            EditItem::Redo => Some(OsAction::Redo),
            EditItem::Cut => Some(OsAction::Cut),
            EditItem::Copy => Some(OsAction::Copy),
            EditItem::Paste => Some(OsAction::Paste),
            EditItem::SelectAll => Some(OsAction::SelectAll),
            EditItem::Delete | EditItem::PasteAndMatchStyle => None,
        }
    }

    /// The key the menu shows, as the kit binds it (`menus` for the specs).
    /// Delete has none in the Swift app's menu.
    pub(crate) fn key(self) -> Option<&'static str> {
        let mac = cfg!(target_os = "macos");
        Some(match self {
            EditItem::Undo => if mac { "⌘Z" } else { "Ctrl+Z" },
            EditItem::Redo => if mac { "⇧⌘Z" } else { "Ctrl+Y" },
            EditItem::Cut => if mac { "⌘X" } else { "Ctrl+X" },
            EditItem::Copy => if mac { "⌘C" } else { "Ctrl+C" },
            EditItem::Paste => if mac { "⌘V" } else { "Ctrl+V" },
            EditItem::PasteAndMatchStyle => if mac { "⌥⇧⌘V" } else { "Ctrl+Shift+V" },
            EditItem::SelectAll => if mac { "⌘A" } else { "Ctrl+A" },
            EditItem::Delete => return None,
        })
    }
}

/// The main window, so a Close from the menu or ⌘W off a Mac knows it's
/// the one that quits.
#[cfg_attr(target_os = "macos", allow(dead_code))]
struct MainWindow(AnyWindowHandle);
impl Global for MainWindow {}

/// Set up once the main window is open: its close quits, and File's and
/// Window's commands reach whichever window is in front, the Settings
/// window and a panel's own window as much as the main one. The main
/// window's own handlers (shell.rs) run these too; these listeners are for
/// the windows that have none.
pub(crate) fn install(main: &mut Window, cx: &mut App) {
    cx.set_global(MainWindow(main.window_handle()));
    // Not then and there: the system is asking the window, and quitting
    // runs the app's quit handlers (drafts saved, the log's last line),
    // which need it free.
    main.on_window_should_close(cx, |_, cx| {
        cx.defer(|cx| cx.quit());
        false
    });
    cx.on_action(|_: &crate::shell::CloseWindow, cx| close_front_window(cx));
    cx.on_action(|_: &crate::shell::MinimizeWindow, cx| {
        with_front_window(cx, |window| window.minimize_window());
    });
    cx.on_action(|_: &crate::shell::ZoomWindow, cx| {
        with_front_window(cx, |window| window.zoom_window());
    });
    cx.on_action(|_: &crate::shell::BringAllToFront, cx| bring_all_to_front(cx));
    let paste_plain = if cfg!(target_os = "macos") { "alt-shift-cmd-v" } else { "ctrl-shift-v" };
    cx.bind_keys([KeyBinding::new(paste_plain, crate::composer_spelling::PasteAndMatchStyle, None)]);
}

fn with_front_window(cx: &mut App, f: impl FnOnce(&mut Window)) {
    if let Some(handle) = cx.active_window() {
        handle.update(cx, |_, window, _| f(window)).ok();
    }
}

/// File > Close (⌘W) from a window with no handler of its own: the one in
/// front (`close_window`).
pub(crate) fn close_front_window(cx: &mut App) {
    if let Some(front) = cx.active_window() {
        front.update(cx, |_, window, cx| close_window(window, cx)).ok();
    }
}

/// File > Close (⌘W): the window closes as its red button would, so each
/// does what it does on closing (Settings saves what's typed, a panel's
/// window puts the panel away, the main window quits).
pub(crate) fn close_window(window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        // `performClose:` asks GPUI's own window whether it may close, which
        // needs the app this runs inside, so it's sent once this is over.
        let _ = cx;
        if let Some(ns_window) = ns_window(window) {
            after_this(&*ns_window, objc2::sel!(performClose:));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Off the Mac there's no asking the window as its button does, so
        // the main window quits and another just goes.
        if cx.try_global::<MainWindow>().is_some_and(|m| m.0 == window.window_handle()) {
            cx.quit();
        } else {
            window.remove_window();
        }
    }
}

/// The window's own NSWindow, as an object to send to.
#[cfg(target_os = "macos")]
pub(crate) fn ns_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
    // SAFETY: the window's content view, alive while the window is, which
    // it is for this call.
    let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
    view.window()
}

/// Sends `action:` (with no sender) to `target` from the run loop's next
/// turn, outside the GPUI update this is called in: AppKit calls back into
/// GPUI as it does it (asking a window to close, a window coming forward),
/// and GPUI's app is busy until this update ends.
#[cfg(target_os = "macos")]
fn after_this<T: objc2::Message>(target: &T, action: objc2::runtime::Sel) {
    let none: Option<&objc2::runtime::AnyObject> = None;
    // SAFETY: NSObject's performSelector:withObject:afterDelay:, with a
    // selector the target answers and a nil argument.
    let _: () = unsafe { objc2::msg_send![target, performSelector: action, withObject: none, afterDelay: 0.0f64] };
}

/// Window > Bring All to Front: every one of the app's windows over other
/// apps', in the order they were (`arrangeInFront:`).
pub(crate) fn bring_all_to_front(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        let _ = cx;
        let Some(mtm) = MainThreadMarker::new() else { return };
        after_this(&*NSApplication::sharedApplication(mtm), objc2::sel!(arrangeInFront:));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let front = cx.active_window();
        let windows = cx.windows();
        for handle in windows.into_iter().filter(|w| Some(*w) != front) {
            handle.update(cx, |_, window, _| window.activate_window()).ok();
        }
        if let Some(front) = front {
            front.update(cx, |_, window, _| window.activate_window()).ok();
        }
    }
}

/// What AppKit put in a menu of the menu bar that this app didn't (AutoFill,
/// Start Dictation, Emoji & Symbols in Edit; Fill, Center and the windows in
/// Window; Enter Full Screen in View), as the menu reads now, for the specs'
/// `menus`: the Swift app's comes from the real menu bar, which has them.
#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn system_items(menu: &str, ours: &[String]) -> Vec<String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else { return Vec::new() };
    let mut out = Vec::new();
    for top in bar.itemArray().iter() {
        let Some(sub) = top.submenu() else { continue };
        if sub.title().to_string() != menu {
            continue;
        }
        // Not `update()`: that asks GPUI to check each item, which needs the
        // app this runs inside.
        for item in sub.itemArray().iter() {
            if item.isSeparatorItem() || item.isHidden() {
                continue;
            }
            let title = item.title().to_string();
            if !title.is_empty() && !ours.contains(&title) {
                out.push(title);
            }
        }
    }
    out
}

#[cfg(all(feature = "automation", not(target_os = "macos")))]
pub(crate) fn system_items(_menu: &str, _ours: &[String]) -> Vec<String> {
    Vec::new()
}
