// Notifications for threads that come to need Mike while he's elsewhere, and
// the Dock badge's count of the threads waiting on him: Swift's Notifier and
// DockBadge. Clicking a notification brings the app forward on that thread.
//
// A scripted app (the automation build the specs drive) shows neither, as
// the Swift app's dev and screenshot runs don't: they'd land on Mike's
// screen, and in his Dock, from apps he isn't using.
use crate::app::Threadbear;
use gpui_kit::*;
use threadbear_core::inbox::{self, ThreadSummary};

pub(crate) fn enabled() -> bool {
    !cfg!(feature = "automation")
}

/// Posts one for `thread` (`Notifier.post`). gpui asks macOS for permission
/// the first time, and posts nothing outside an app bundle (a `cargo run`).
pub(crate) fn post(thread: &ThreadSummary, cx: &App) {
    if !enabled() {
        return;
    }
    let (title, body) = inbox::notification_words(thread);
    cx.show_system_notification(SystemNotification { tag: inbox::notification_tag(thread).into(), title: title.into(), body: body.into(), actions: vec![] });
}

/// A click on one opens its thread in the main window, once the list has
/// loaded if the click is what started the app (as a link does).
pub(crate) fn install(app: WeakEntity<Threadbear>, window: AnyWindowHandle, cx: &mut App) {
    if !enabled() {
        return;
    }
    cx.on_system_notification_response(move |response, cx| {
        let Some(id) = inbox::thread_in_tag(&response.tag).map(str::to_owned) else { return };
        cx.activate(true);
        let app = app.clone();
        window
            .update(cx, |_, window, cx| {
                app.update(cx, |this, cx| this.open_from_outside(id, window, cx)).ok();
            })
            .ok();
    });
}

/// The badge: how many threads need him, nothing when none do
/// (`refreshDockBadge`). Only set when it changes.
pub(crate) fn set_badge(label: Option<&str>) {
    if !enabled() {
        return;
    }
    platform::set_badge(label);
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    pub(super) fn set_badge(label: Option<&str>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        tile.setBadgeLabel(label.map(NSString::from_str).as_deref());
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    // Windows' taskbar overlay and Linux's launcher counts are each their
    // own thing; none yet.
    pub(super) fn set_badge(_label: Option<&str>) {}
}
