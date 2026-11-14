// The sidebar's long lists (the Inbox's threads, Projects) scroll as plain
// divs, so every row was built and laid out again on each frame of a scroll:
// GPUI redraws the view a scroll lands in, and a cached view can't be kept
// for a list that's moving under it (its key is where it is). With Mike's
// 80 projects that was most of a frame. Rows well out of view are drawn as a
// gap their height instead, measured when they were last drawn, and built
// again as a scroll brings them near, a screen's height before they show.
//
// A row's height doesn't change while it's out of view (its words are one
// line each, cut short), so the list doesn't move as rows swap in and out.
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// How far past the view, above and below, rows are still built: at least
/// this much, and a whole view's height on a tall one, so a fast fling
/// reaches built rows a few frames before they show.
const MARGIN: f32 = 400.;

/// One list's rows: where each was last laid out, from the list's top.
#[derive(Default)]
pub(crate) struct FarRows {
    list_top: Cell<f32>,
    placed: RefCell<HashMap<String, (f32, f32)>>,
    /// The view to draw again when a gap turns out to be in view after all
    /// (the list changed under it), and the list's scroll.
    view: Cell<Option<EntityId>>,
    scroll: RefCell<Option<ScrollHandle>>,
    /// The rows drawn this frame, so a row that's gone is forgotten (and if
    /// it comes back, somewhere else, it's built where it now is).
    drawn: RefCell<HashSet<String>>,
    /// The zoom the heights were measured at: a new zoom makes every row
    /// another height, so they're all measured again (Codex's review).
    zoom: Cell<u32>,
}

impl FarRows {
    /// Starts a frame of the list: the view it's drawn in and its scroll.
    /// Rows not drawn in the last frame are forgotten.
    pub(crate) fn begin(&self, view: Option<EntityId>, scroll: &ScrollHandle) {
        self.view.set(view);
        *self.scroll.borrow_mut() = Some(scroll.clone());
        let drawn = std::mem::take(&mut *self.drawn.borrow_mut());
        self.placed.borrow_mut().retain(|k, _| drawn.contains(k));
        let zoom = crate::style::zoom_percent();
        if self.zoom.replace(zoom) != zoom {
            self.placed.borrow_mut().clear();
        }
    }

    /// Notes where the list is: the list's first child, so it's laid out
    /// before any row.
    pub(crate) fn marker(self: &Rc<Self>) -> impl IntoElement {
        let rows = self.clone();
        canvas(move |bounds, _, _| rows.list_top.set(f32::from(bounds.origin.y)), |_, _, _, _| {}).absolute().top_0().left_0().size_0()
    }

    /// The row's height when it's far enough out of view to be a gap: it
    /// was laid out before, and both where it was and the view are known.
    pub(crate) fn far(&self, key: &str) -> Option<f32> {
        let (top, height) = *self.placed.borrow().get(key)?;
        let scroll = self.scroll.borrow();
        let scroll = scroll.as_ref()?;
        is_far(top, height, scroll).then_some(height)
    }

    /// The row, or for a far one (`far` said its height) a gap that tall,
    /// noting where it's laid out this frame.
    pub(crate) fn row(self: &Rc<Self>, key: &str, row: Option<AnyElement>, gap: f32) -> AnyElement {
        self.drawn.borrow_mut().insert(key.to_owned());
        let is_gap = row.is_none();
        let child = row.unwrap_or_else(|| div().w_full().h(gpui_kit::px(gap)).into_any_element());
        Placed { key: key.to_owned(), child, rows: self.clone(), is_gap }.into_any_element()
    }
}

fn is_far(top: f32, height: f32, scroll: &ScrollHandle) -> bool {
    let tall = f32::from(scroll.bounds().size.height);
    if tall <= 0. {
        return false;
    }
    let seen = -f32::from(scroll.offset().y);
    let margin = MARGIN.max(tall);
    top + height < seen - margin || top > seen + tall + margin
}

/// A row that notes where it's laid out, from the list's top.
struct Placed {
    key: String,
    child: AnyElement,
    rows: Rc<FarRows>,
    is_gap: bool,
}

impl IntoElement for Placed {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Placed {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        let top = f32::from(bounds.origin.y) - self.rows.list_top.get();
        let height = f32::from(bounds.size.height);
        self.rows.placed.borrow_mut().insert(self.key.clone(), (top, height));
        // A gap that's come into view: the list changed above it (a section
        // opened) since it was measured. Draw the view again, with the row.
        if self.is_gap {
            let near = self.rows.scroll.borrow().as_ref().is_some_and(|s| !is_far(top, height, s));
            if near {
                if let Some(view) = self.rows.view.get() {
                    cx.notify(view);
                } else {
                    window.refresh();
                }
            }
        }
        self.child.prepaint(window, cx);
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}
