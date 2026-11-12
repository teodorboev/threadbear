// A question that has to be answered before anything goes on (an NSAlert, as
// the Swift app asks), for the few places gpui's own prompt can't be used.
// Shown from a task, outside the app's update, as its loop runs until it's
// answered and the windows go on drawing behind it.

/// Asks `title`, with `detail` under it; true for `ok`. A scripted app never
/// asks (no one would answer) and takes `cancel`.
pub(crate) fn confirm(title: &str, detail: &str, ok: &str, cancel: &str) -> bool {
    if cfg!(feature = "automation") {
        return false;
    }
    platform::confirm(title, detail, ok, cancel)
}

#[cfg(target_os = "macos")]
mod platform {
    pub(super) fn confirm(title: &str, detail: &str, ok: &str, cancel: &str) -> bool {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn};
        use objc2_foundation::NSString;
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(detail));
        alert.addButtonWithTitle(&NSString::from_str(ok));
        alert.addButtonWithTitle(&NSString::from_str(cancel));
        alert.runModal() == NSAlertFirstButtonReturn
    }
}

// Elsewhere it doesn't go ahead, until there's a dialog of the app's own here.
#[cfg(not(target_os = "macos"))]
mod platform {
    pub(super) fn confirm(_title: &str, _detail: &str, _ok: &str, _cancel: &str) -> bool {
        false
    }
}
