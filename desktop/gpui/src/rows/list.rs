// The transcript's list (TranscriptController.swift): GPUI's `list` following
// its tail, with the anchoring it already does across inserts and opened rows,
// plus what the Swift controller keeps track of: whether the view is at the
// bottom, the row at the top for scroll memory, and the row sizes `dump`
// reports. MessageScroller was close but hides the list state and has a jump
// button and bottom fade the Swift app doesn't.
use super::metrics::{FADE_HEIGHT, LIST_TOP_INSET, TOP_FADE_HEIGHT};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::ops::Range;

/// How far past the viewport the list lays rows out.
const OVERDRAW: Pixels = gpui_kit::px(600.);

pub struct TranscriptList {
    list: ListState,
}

impl TranscriptList {
    pub fn new(count: usize, cx: &mut Context<Self>) -> Self {
        let list = ListState::new(count, ListAlignment::Top, OVERDRAW);
        list.set_follow_mode(FollowMode::Tail);
        let weak = cx.weak_entity();
        list.set_scroll_handler(move |_, _, cx| {
            let weak = weak.clone();
            // The list holds its own borrow while it scrolls; notify after.
            cx.defer(move |cx| {
                let _ = weak.update(cx, |_, cx| cx.notify());
            });
        });
        TranscriptList { list }
    }

    pub fn reset(&mut self, count: usize, cx: &mut Context<Self>) {
        self.list.reset(count);
        self.list.set_follow_mode(FollowMode::Tail);
        cx.notify();
    }

    pub fn splice(&mut self, range: Range<usize>, count: usize, cx: &mut Context<Self>) -> bool {
        if range.start > range.end || range.end > self.list.item_count() {
            return false;
        }
        self.list.splice(range, count);
        cx.notify();
        true
    }

    /// Rows added at the top with what's on screen kept where it is.
    pub fn prepend(&mut self, count: usize, cx: &mut Context<Self>) -> bool {
        self.splice(0..0, count, cx)
    }

    pub fn remeasure_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> bool {
        if range.start > range.end || range.end > self.list.item_count() {
            return false;
        }
        self.list.remeasure_items(range);
        cx.notify();
        true
    }

    /// `scrollToBottom`, and following again from there.
    pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        cx.notify();
    }

    /// `scroll(toRow:offset:)`: row `index` at `offset` below the top of the view.
    pub fn scroll_to_row(&mut self, index: usize, offset: Pixels, cx: &mut Context<Self>) {
        // A positive offset puts the row below the top of the view: the list
        // can't start a row part way down, so it starts a little earlier.
        let (index, offset_in_item) = if offset >= px(0.) { (index, px(0.)) } else { (index, -offset) };
        self.list.scroll_to(ListOffset { item_ix: index, offset_in_item });
        cx.notify();
    }

    /// Moves the view up by `distance`, so a row just put at the top sits
    /// that far down (a work link's step, a third of the way down, as the
    /// Swift `scroll(toRow:offset:)` puts it). Only rows that have been laid
    /// out have heights, so call it a frame after the row's been scrolled to:
    /// the list's overdraw has measured the rows just above it by then.
    pub fn back_up(&mut self, distance: Pixels, cx: &mut Context<Self>) {
        self.list.scroll_by(-distance);
        cx.notify();
    }

    /// While the mouse button's down on a selection: stop following new
    /// lines, so the words don't move under the pointer. Whether it was
    /// following, for `resume_after_selection`.
    pub fn pause_for_selection(&mut self) -> bool {
        let following = self.list.is_following_tail();
        if following {
            // Normal, not GPUI's pause: a pause takes up following again the
            // moment the view's at the end, which it is as the drag starts.
            self.list.set_follow_mode(FollowMode::Normal);
        }
        following
    }

    /// Whether it's following new lines right now (for scripts).
    pub fn following(&self) -> bool {
        self.list.is_following_tail()
    }

    pub fn viewport(&self) -> Bounds<Pixels> {
        self.list.viewport_bounds()
    }

    /// `isAtBottom`: following the tail. Rows above the view that have never
    /// been laid out count as zero height, so anything computed from pixels
    /// is unreliable until they have been; GPUI restarts following the moment
    /// the view really reaches the end, which is what this needs to know.
    pub fn at_bottom(&self) -> bool {
        self.list.is_following_tail() || self.list.is_scrolled_to_end() == Some(true)
    }

    /// The row's height once it's been measured, for `dump`.
    #[cfg(feature = "automation")]
    pub fn row_height(&self, index: usize) -> Option<Pixels> {
        self.list.bounds_for_item(index).map(|b| b.size.height)
    }

    /// Where the row sits in the window, once measured, for `dump`.
    #[cfg(feature = "automation")]
    pub fn row_bounds(&self, index: usize) -> Option<Bounds<Pixels>> {
        self.list.bounds_for_item(index)
    }

    /// Whether the row is in the viewport right now, for `dump`.
    #[cfg(feature = "automation")]
    pub fn row_on_screen(&self, index: usize) -> bool {
        let Some(bounds) = self.list.bounds_for_item(index) else { return false };
        let viewport = self.list.viewport_bounds();
        bounds.bottom() > viewport.top() && bounds.top() < viewport.bottom()
    }

    /// The row at the top of the viewport and how far down it the view starts.
    pub fn top_row(&self) -> Option<(usize, Pixels)> {
        let top = self.list.logical_scroll_top();
        if top.item_ix < self.list.item_count() {
            Some((top.item_ix, top.offset_in_item))
        } else {
            None
        }
    }
}

/// The list and its overlays: an overlay scrollbar, the top and bottom fades
/// (`updateFade`: rows slide under them rather than being cut off), and the
/// inset at the end of the rows so a pinned thread's last row sits clear of
/// the bottom fade.
pub fn transcript_list(
    state: &Entity<TranscriptList>,
    background: Hsla,
    mut render_row: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
    cx: &App,
) -> impl IntoElement {
    let list_state = state.read(cx).list.clone();
    let count = list_state.item_count();
    let rows = list(list_state.clone(), move |index, window, cx| {
        // The top inset rides on the first row and the bottom one on the
        // last, so the list's scroll range covers them.
        div()
            .w_full()
            .when(index == 0, |d| d.pt(px(LIST_TOP_INSET)))
            .when(index + 1 == count, |d| d.pb(px(FADE_HEIGHT + 4.)))
            .child(render_row(index, window, cx))
            .into_any_element()
    })
    .size_full()
    .min_h_0();
    let weak = state.downgrade();
    div()
        .id("transcript")
        .relative()
        .size_full()
        .min_h_0()
        .min_w_0()
        .overflow_hidden()
        .child(
            div().id("transcript-viewport").size_full().min_h_0().min_w_0().child(base::ScrollBounce::new("transcript-bounce", &list_state, rows).on_scroll(move |_, cx| {
                let _ = weak.update(cx, |_, cx| cx.notify());
            })),
        )
        // Rows slide under the fades instead of being cut off, and the scroll
        // bar's strip on the right stays whole.
        .child(fade(background, FadeEdge::Top))
        .child(fade(background, FadeEdge::Bottom))
        .child(div().absolute().inset_0().child(div().size_full().vertical_scrollbar(&list_state)))
}

enum FadeEdge {
    Top,
    Bottom,
}

/// `updateFade`'s mask drawn as an overlay of the pane's colour. The bottom
/// eases: most of the fall happens over the first part, so the last few points
/// above the composer are nearly clear of it.
fn fade(background: Hsla, edge: FadeEdge) -> Div {
    let solid = background;
    let clear = background.opacity(0.);
    match edge {
        FadeEdge::Top => div().absolute().top_0().left_0().right(px(16.)).h(px(TOP_FADE_HEIGHT)).bg(linear_gradient(180., linear_color_stop(solid, 0.), linear_color_stop(clear, 1.))),
        FadeEdge::Bottom => {
            let heavy = FADE_HEIGHT * 0.65;
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right(px(16.))
                .h(px(FADE_HEIGHT))
                .flex()
                .flex_col()
                .child(div().w_full().h(px(heavy)).bg(linear_gradient(180., linear_color_stop(clear, 0.), linear_color_stop(background.opacity(0.65), 1.))))
                .child(div().w_full().flex_1().bg(linear_gradient(180., linear_color_stop(background.opacity(0.65), 0.), linear_color_stop(solid, 1.))))
        }
    }
}
