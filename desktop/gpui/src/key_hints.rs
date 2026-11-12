// Holding ⌘ shows each button's key beside it, as the Swift app's
// ShortcutHints does (from Tandem), so the keys can be learned at a glance
// rather than printed on the buttons all the time. The keys show the moment
// ⌘ goes down on its own and go the moment it's let go or used: a shortcut,
// a ⌘-click or a ⌘-scroll hides them, and they don't come back until ⌘ is
// pressed again. Control plays ⌘'s part off the Mac, as in the shortcuts.
use crate::app::Threadbear;
use crate::style::{self, px};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::Cell;

thread_local! {
    /// The keys are on show.
    static SHOWING: Cell<bool> = const { Cell::new(false) };
    /// ⌘ was used for something since it went down.
    static SPENT: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn showing() -> bool {
    SHOWING.with(Cell::get)
}

/// Where a key goes against its button (`ShortcutHintPlacement`).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Placement {
    /// Just right of it, over whatever's there (the activity bar's icons).
    After,
    /// Just left of it (New thread at the sidebar's edge, the composer's Stop).
    Before,
}

/// The modifier the shortcuts use here.
fn command_held(modifiers: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    }
}

/// ⌘ on its own, with nothing else held.
fn command_alone(modifiers: &Modifiers) -> bool {
    let mac = cfg!(target_os = "macos");
    command_held(modifiers) && !modifiers.shift && !modifiers.alt && !modifiers.function && if mac { !modifiers.control } else { !modifiers.platform }
}

/// Watches every key the app takes, before it's acted on: a shortcut is
/// handled by its action and never reaches the window's key listeners, and
/// a key with ⌘ held is ⌘ used for something.
pub(crate) fn install(app: WeakEntity<Threadbear>, cx: &mut App) {
    cx.intercept_keystrokes(move |event, _, cx| {
        let modifiers = event.keystroke.modifiers;
        app.update(cx, |this, cx| this.hints_used(&modifiers, cx)).ok();
    })
    .detach();
}

impl Threadbear {
    /// A modifier went down or up (`ShortcutHints.handle`'s flagsChanged).
    pub(crate) fn hints_modifiers_changed(&mut self, modifiers: &Modifiers, window: &Window, cx: &mut Context<Self>) {
        if !command_held(modifiers) {
            SPENT.with(|s| s.set(false));
            self.set_hints(false, cx);
        } else if command_alone(modifiers) {
            if !SPENT.with(Cell::get) && window.is_window_active() {
                self.set_hints(true, cx);
            }
        } else {
            // ⌘ with Shift or Option: a chord on its way.
            SPENT.with(|s| s.set(true));
            self.set_hints(false, cx);
        }
    }

    /// A key, a click or a scroll: whatever ⌘ is down for, it isn't for
    /// looking at the keys.
    pub(crate) fn hints_used(&mut self, modifiers: &Modifiers, cx: &mut Context<Self>) {
        if command_held(modifiers) {
            SPENT.with(|s| s.set(true));
        }
        self.set_hints(false, cx);
    }

    /// The window went to the back (⌘-Tab lets ⌘ go in the other app).
    pub(crate) fn hints_reset(&mut self, cx: &mut Context<Self>) {
        SPENT.with(|s| s.set(false));
        self.set_hints(false, cx);
    }

    pub(crate) fn set_hints(&mut self, on: bool, cx: &mut Context<Self>) {
        if showing() != on {
            SHOWING.with(|s| s.set(on));
            // The buttons draw them, the sidebar's in a view of its own.
            cx.notify();
        }
    }

    /// The automation's `hints`: the keys as if ⌘ were held, or let go, as
    /// the Swift command sets them (a script can't hold a key down).
    #[cfg(feature = "automation")]
    pub(crate) fn hints_command(&mut self, on: bool, cx: &mut Context<Self>) {
        SPENT.with(|s| s.set(false));
        self.set_hints(on, cx);
    }

    /// The keys on show, as "Threads ⌘1", in the order the buttons are,
    /// for the dump (the Swift dump says the same of its buttons).
    #[cfg(feature = "automation")]
    pub(crate) fn key_hints_dump(&self, cx: &App) -> Vec<String> {
        use threadbear_core::shortcuts::ShortcutCommand as C;
        if !showing() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut add = |title: &str, command: C| {
            if let Some(key) = self.shortcut_display(command) {
                out.push(format!("{title} {key}"));
            }
        };
        add("Threads", C::ShowInbox);
        if self.schedules_supported {
            add("Scheduled", C::ShowScheduled);
        }
        if self.projects_supported {
            add("Projects", C::ShowProjects);
            add("Librarian", C::ShowLibrarian);
        }
        add("Settings", C::Settings);
        // Beside the search, in the Inbox's list.
        if self.mode == crate::shell::SidebarMode::Inbox && self.threads_showing(cx) {
            add("New thread", C::NewThread);
        }
        // The composer's Stop, while its thread runs with nothing typed.
        let stop = self.open_thread().is_some_and(|t| t.busy()) && self.composer_is_empty(cx);
        if stop {
            add("Stop", C::Stop);
        }
        out
    }

    #[cfg(feature = "automation")]
    fn composer_is_empty(&self, cx: &App) -> bool {
        self.composers.get(self.selected.as_deref().unwrap_or_default()).is_some_and(|c| c.read(cx).draft_text(cx).trim().is_empty() && c.read(cx).draft_images(cx) == 0)
    }
}

/// The key beside a button while ⌘ is held, or nothing. The button has to
/// be `relative`; the key hangs off its edge over its neighbours, so it's
/// drawn after everything else (`zIndex` in the Swift app).
pub(crate) fn hint(key: Option<String>, placement: Placement) -> Option<AnyElement> {
    if !showing() {
        return None;
    }
    let key = key?;
    let holder = div().absolute().top_0().bottom_0().flex().items_center();
    let holder = match placement {
        Placement::After => holder.left_full().pl(px(4.)),
        Placement::Before => holder.right_full().pr(px(4.)),
    };
    Some(deferred(holder.child(badge(key))).with_priority(3).into_any_element())
}

/// A key as the hints draw it: dark text on amber, Tandem's colours, so it
/// reads as a passing label over the window rather than part of it
/// (`ShortcutKeyBadge`).
fn badge(key: String) -> impl IntoElement {
    let amber = rgb(0xFFB224);
    let on_amber = rgb(0x0C0D0F);
    div()
        .flex_none()
        .px(px(4.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(amber)
        .text_color(on_amber)
        .text_size(px(10.5))
        .line_height(px(style::natural_line(10.5)))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .shadow(vec![BoxShadow::new(gpui_kit::px(0.), px(1.), hsla(0., 0., 0., 0.35)).blur_radius(px(2.))])
        .child(key)
}
