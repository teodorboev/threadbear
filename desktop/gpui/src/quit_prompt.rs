// ⌘Q and Quit Threadbear ask first, so a stray ⌘Q doesn't close the app
// (Swift's QuitPrompt), with Don't ask again, which Settings > General >
// Quitting turns back on. The threads keep running on the runner either way.
use crate::app::Threadbear;
use gpui_kit::*;

pub(crate) struct Answer {
    pub(crate) quit: bool,
    pub(crate) dont_ask_again: bool,
}

/// Quits, asking first unless he's said not to (`asks`, from the app's
/// settings: it's passed in, as this runs inside the app's own update).
pub(crate) fn quit(asks: bool, app: WeakEntity<Threadbear>, cx: &mut App) {
    // A scripted app never asks: an alert would sit there with no one to
    // answer it (the Swift app's tests swap the alert out for the same reason).
    if cfg!(feature = "automation") || !asks {
        cx.quit();
        return;
    }
    // The alert runs its own loop until it's answered, so it's shown from a
    // task, outside the app's update: windows go on drawing behind it.
    cx.spawn(async move |cx| {
        cx.update(|cx| cx.activate(true));
        let answer = ask();
        if !answer.quit {
            return;
        }
        cx.update(|cx| {
            if answer.dont_ask_again {
                app.update(cx, |app, _| {
                    app.settings.saved.ask_before_quitting = Some(false);
                    app.settings.save();
                })
                .ok();
            }
            cx.quit();
        });
    })
    .detach();
}

#[cfg(target_os = "macos")]
fn ask() -> Answer {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSControlStateValueOn};
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else { return Answer { quit: true, dont_ask_again: false } };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Quit Threadbear?"));
    alert.setInformativeText(&NSString::from_str("Your threads keep running on the runner while Threadbear is closed."));
    alert.addButtonWithTitle(&NSString::from_str("Quit"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.setShowsSuppressionButton(true);
    let suppression = alert.suppressionButton();
    if let Some(button) = &suppression {
        button.setTitle(&NSString::from_str("Don't ask again"));
    }
    let quit = alert.runModal() == NSAlertFirstButtonReturn;
    let dont_ask_again = suppression.is_some_and(|b| b.state() == NSControlStateValueOn);
    Answer { quit, dont_ask_again }
}

// Elsewhere it quits as asked, until there's a dialog of the app's own here.
#[cfg(not(target_os = "macos"))]
fn ask() -> Answer {
    Answer { quit: true, dont_ask_again: false }
}
