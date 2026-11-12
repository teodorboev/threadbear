// The app's text fields that hold words not saved anywhere yet, so an
// update's restart by itself waits for them (app_update.rs). Each field is
// put on the list where it's made (`watch`), with what it holds then, and
// says what it holds now when asked: one whose words differ from that, or
// from what was last saved (`saved`), holds the restart off. A field that's
// gone (its editor closed, its card answered) drops off by itself.
//
// Not on the list, on purpose: the message boxes (app_update.rs reads their
// drafts, pictures still coming in included), the search fields (a search
// left typed in would hold every update off, and is quick to type again),
// Settings' standing instructions (saved as they're typed), a review's
// reply (kept on this computer through a restart) and the schedule editor
// (open at all holds it). Codex's review of the restart, 12 Nov: the
// project page's boxes, a card's Something else and a secret's field were
// missed.
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;
use std::cell::RefCell;

/// A field that can say what's typed in it.
pub(crate) trait Typed: 'static {
    fn typed(&self) -> String;
}

impl Typed for InputState {
    fn typed(&self) -> String {
        self.value().to_string()
    }
}

impl Typed for TextareaState {
    fn typed(&self) -> String {
        self.value().to_string()
    }
}

struct Watched {
    id: EntityId,
    /// What it says now, or none once it's gone.
    now: Box<dyn Fn(&App) -> Option<String>>,
    saved: String,
}

thread_local! {
    static WATCHED: RefCell<Vec<Watched>> = const { RefCell::new(Vec::new()) };
}

/// Puts a new field on the list, with what it holds now as saved.
pub(crate) fn watch<T: Typed>(input: &Entity<T>, cx: &App) {
    let saved = input.read(cx).typed();
    let weak = input.downgrade();
    let now = Box::new(move |cx: &App| weak.upgrade().map(|i| i.read(cx).typed()));
    WATCHED.with(|w| w.borrow_mut().push(Watched { id: input.entity_id(), now, saved }));
}

/// What the field holds now is saved (or is what's there to begin with):
/// a field that follows a value from elsewhere says so when it takes it.
pub(crate) fn saved<T: Typed>(input: &Entity<T>, cx: &App) {
    let text = input.read(cx).typed();
    WATCHED.with(|w| {
        if let Some(entry) = w.borrow_mut().iter_mut().find(|e| e.id == input.entity_id()) {
            entry.saved = text;
        }
    });
}

/// Whether any field on the list holds words not saved. Fields that have
/// gone are dropped as it looks.
pub(crate) fn any(cx: &App) -> bool {
    WATCHED.with(|w| {
        let mut w = w.borrow_mut();
        let mut typed = false;
        w.retain(|entry| match (entry.now)(cx) {
            Some(now) => {
                typed |= threadbear_core::app_update::typed_since(&entry.saved, &now);
                true
            }
            None => false,
        });
        typed
    })
}
