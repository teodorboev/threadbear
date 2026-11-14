// The sidebar's rows gliding to where they are now when the list changes (a
// thread moving from Running to the Inbox, a new one arriving at the top),
// rather than jumping there, and its counts rolling to a new number: the
// Swift app's `Juice.rows` over rows matched in the list's space
// (`sidebarRowSpace`), and `contentTransition(.numericText)` on the badges
// (Juice.swift, SidebarView's ThreadList and SectionHeading, ActivityBar).
//
// GPUI has no matched geometry, so this is done the FLIP way: each row is
// laid out where it is now and, for a moment, painted where it was, the
// difference easing away (threadbear_core::row_motion keeps the books). It's
// measured from the list's own top, so scrolling never reads as a move.
use crate::style::px;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use threadbear_core::juice;
use threadbear_core::row_motion::{Glides, ARRIVE};

/// A row turning up comes down this far as it fades in (`Juice.row`'s grow
/// from its top edge, as near as GPUI, which can't scale a row, gets).
const ARRIVE_DROP: f32 = 4.;
/// A number rolling to its new value (`Juice.swap`: snappy, 0.25 s).
const ROLL: Duration = Duration::from_millis(250);

/// One list's rows in motion. Kept by the sidebar (`SidebarState::motion`)
/// and shared with the elements that draw them.
#[derive(Default)]
pub(crate) struct RowMotion {
    /// The list's top in the window this frame.
    list_top: Cell<f32>,
    glides: RefCell<Glides>,
    /// Where each row was laid out this frame, from the list's top: its top
    /// and height, for scrolling one into view (`reveal`).
    placed: RefCell<HashMap<String, (f32, f32)>>,
    /// Where each row was painted this frame, in the window, glide and all:
    /// what the selection's highlight slides between.
    painted: RefCell<HashMap<String, Bounds<Pixels>>>,
    /// The selected row, and the highlight sliding to it from the one before.
    selection: RefCell<Selection>,
}

#[derive(Default)]
struct Selection {
    key: Option<String>,
    /// From which row, and when it set off.
    slide: Option<(String, Instant)>,
}

/// A row to scroll into view once it's been laid out, and how many frames
/// it's been waited for (a folded section opening, the list loading).
pub(crate) type Reveal = Rc<RefCell<Option<(String, u8)>>>;

/// How much of the list's foot the fade covers: a row under it isn't in view.
const FOOT_FADE: f32 = 34.;
/// Room left above or below a row scrolled into view.
const REVEAL_MARGIN: f32 = 4.;

impl RowMotion {
    /// The rows about to be drawn, by key, top to bottom (`Glides::rows_drawn`).
    pub(crate) fn rows_drawn(&self, rows: Vec<String>, quiet: &HashSet<String>, reduce: bool) {
        self.placed.borrow_mut().retain(|k, _| rows.contains(k));
        self.painted.borrow_mut().retain(|k, _| rows.contains(k));
        self.glides.borrow_mut().rows_drawn(rows, quiet, reduce, Instant::now());
    }

    /// Whether the row is laid out within the part of the list that's showing.
    pub(crate) fn in_view(&self, key: &str, scroll: &ScrollHandle) -> Option<bool> {
        let (top, height) = *self.placed.borrow().get(key)?;
        let seen = -f32::from(scroll.offset().y);
        let tall = f32::from(scroll.bounds().size.height) - FOOT_FADE;
        Some(top >= seen - 0.5 && top + height <= seen + tall + 0.5)
    }

    /// The last thing the list draws: once the rows above it are laid out this
    /// frame, scrolls the one `reveal` names into view, as little as it takes
    /// (SwiftUI's `scrollTo` with no anchor), and has `view` drawn again for
    /// it. A row not laid out yet is looked for again a few frames running.
    pub(crate) fn reveal_marker(self: &Rc<Self>, reveal: Reveal, scroll: ScrollHandle, view: Option<EntityId>) -> impl IntoElement {
        let motion = self.clone();
        canvas(
            move |_, window, cx| {
                let Some((key, tries)) = reveal.borrow().clone() else { return };
                let placed = motion.placed.borrow().get(&key).copied();
                let redraw = |cx: &mut App| {
                    if let Some(view) = view {
                        cx.notify(view);
                    }
                };
                match placed {
                    Some((top, height)) => {
                        *reveal.borrow_mut() = None;
                        let offset = scroll.offset();
                        let seen = -f32::from(offset.y);
                        let tall = (f32::from(scroll.bounds().size.height) - FOOT_FADE).max(height);
                        let to = if top < seen {
                            top - REVEAL_MARGIN
                        } else if top + height > seen + tall {
                            top + height - tall + REVEAL_MARGIN
                        } else {
                            return;
                        };
                        let most = f32::from(scroll.max_offset().y);
                        scroll.set_offset(point(offset.x, gpui_kit::px(-to.clamp(0., most.max(0.)))));
                        redraw(cx);
                    }
                    None if tries < 20 => {
                        *reveal.borrow_mut() = Some((key, tries + 1));
                        window.request_animation_frame();
                        redraw(cx);
                    }
                    None => *reveal.borrow_mut() = None,
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_0()
    }

    /// Notes where the list is this frame, for rows to measure from: the
    /// list's first child, so it's prepainted before any row.
    pub(crate) fn list_marker(self: &Rc<Self>) -> impl IntoElement {
        let motion = self.clone();
        canvas(move |bounds, _, _| motion.list_top.set(f32::from(bounds.origin.y)), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
    }

    /// Says which row is selected, before the rows are drawn, and whether
    /// its highlight is sliding there from the one before (`Juice.selection`
    /// over the highlight matched across the list, `rowHighlight`): while it
    /// is, the row leaves its own highlight to `selection_marker`. Only from
    /// a row that was on show, and not under Reduce Motion, where it jumps.
    pub(crate) fn selection(&self, key: Option<&str>, reduce: bool) -> bool {
        let mut selection = self.selection.borrow_mut();
        if selection.key.as_deref() != key {
            let from = selection.key.take().filter(|from| !reduce && key.is_some() && self.painted.borrow().contains_key(from));
            selection.slide = from.map(|from| (from, Instant::now()));
            selection.key = key.map(str::to_owned);
        }
        if selection.slide.as_ref().is_some_and(|(_, at)| at.elapsed() >= juice::SELECTION_SLIDE) {
            selection.slide = None;
        }
        selection.slide.is_some()
    }

    /// The highlight on its way from one row to the next, under the rows: the
    /// list's first child but one, so it paints before any of them, once
    /// they've all been placed this frame.
    pub(crate) fn selection_marker(self: &Rc<Self>, color: Hsla, corner: f32) -> impl IntoElement {
        let motion = self.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let selection = motion.selection.borrow();
                let (Some(to), Some((from, at))) = (selection.key.as_ref(), selection.slide.as_ref()) else { return };
                let painted = motion.painted.borrow();
                let (Some(a), Some(b)) = (painted.get(from), painted.get(to)) else { return };
                let t = juice::selection_slide(juice::progress(at.elapsed(), juice::SELECTION_SLIDE));
                let mix = |x: Pixels, y: Pixels| x + (y - x) * t;
                let bounds = Bounds::new(point(mix(a.origin.x, b.origin.x), mix(a.origin.y, b.origin.y)), size(mix(a.size.width, b.size.width), mix(a.size.height, b.size.height)));
                window.paint_quad(fill(bounds, color).corner_radii(px(corner)));
                // Until it lands, and once more after, for the row to take its
                // highlight back.
                window.request_animation_frame();
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// A row that glides from its last place to its new one, and fades in
    /// when it's new.
    pub(crate) fn row(self: &Rc<Self>, key: &str, row: AnyElement) -> AnyElement {
        let arrived = self.glides.borrow_mut().arrived(key, Instant::now());
        let row = match arrived {
            // Keyed by when it arrived, so a row that comes back runs it again.
            Some(at) => div()
                .relative()
                .child(row)
                .with_animation(SharedString::from(format!("arrive-{key}-{at:?}")), Animation::new(ARRIVE).with_easing(ease_out_quint()), |el, t| el.opacity(t).top(px(-ARRIVE_DROP * (1. - t))))
                .into_any_element(),
            None => row,
        };
        Glide { key: key.to_owned(), child: row, motion: self.clone() }.into_any_element()
    }
}

thread_local! {
    /// Numbers rolling, by where they're shown: the one before, the one now,
    /// and when it changed. The window draws on one thread.
    static COUNTS: RefCell<HashMap<String, (usize, usize, Instant)>> = RefCell::new(HashMap::new());
}

/// A count that rolls to its new number as it changes: the old one goes up
/// and out as the new one comes up from below (down for a smaller number),
/// as SwiftUI's numeric text does. `key` says which count it is, `height` is
/// a line of it, and `text` draws one number.
pub(crate) fn rolling(key: &str, value: usize, height: f32, reduce: bool, text: impl Fn(usize) -> AnyElement) -> AnyElement {
    let now = Instant::now();
    let (before, changed) = COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let entry = counts.entry(key.to_owned()).or_insert((value, value, now.checked_sub(ROLL).unwrap_or(now)));
        if entry.1 != value {
            *entry = (entry.1, value, now);
        }
        (entry.0, entry.2)
    });
    if reduce || before == value || changed.elapsed() >= ROLL {
        return text(value);
    }
    // Up for more, down for fewer.
    let way = if value > before { 1. } else { -1. };
    let id = format!("roll-{key}-{before}-{value}-{changed:?}");
    div()
        .relative()
        .overflow_hidden()
        .child(div().relative().child(text(value)).with_animation(SharedString::from(format!("{id}-in")), Animation::new(ROLL).with_easing(ease_out_quint()), move |el, t| {
            el.top(px(height * way * (1. - t))).opacity(t)
        }))
        .child(div().absolute().top_0().left_0().right_0().child(text(before)).with_animation(SharedString::from(format!("{id}-out")), Animation::new(ROLL).with_easing(ease_out_quint()), move |el, t| {
            el.top(px(-height * way * t)).opacity(1. - t)
        }))
        .into_any_element()
}

/// A row, laid out where it is now and painted where its glide has got to.
struct Glide {
    key: String,
    child: AnyElement,
    motion: Rc<RowMotion>,
}

impl IntoElement for Glide {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Glide {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        // The row's own layout: this only moves where it's painted.
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        let top = f32::from(bounds.origin.y) - self.motion.list_top.get();
        self.motion.placed.borrow_mut().insert(self.key.clone(), (top, f32::from(bounds.size.height)));
        let offset = self.motion.glides.borrow_mut().place(&self.key, top, Instant::now());
        if offset != 0. {
            window.request_animation_frame();
        }
        self.motion.painted.borrow_mut().insert(self.key.clone(), Bounds::new(point(bounds.origin.x, bounds.origin.y + gpui_kit::px(offset)), bounds.size));
        let child = &mut self.child;
        window.with_element_offset(point(gpui_kit::px(0.), gpui_kit::px(offset)), |window| {
            child.prepaint(window, cx);
        });
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}
