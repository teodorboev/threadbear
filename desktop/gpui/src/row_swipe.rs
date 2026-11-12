// A sidebar row swiped or dragged (the Swift app's RowSwipe.swift and the
// row's drag gesture): two fingers left across it on a trackpad, or the mouse
// dragging it left, slides it after the pointer with what letting go will do
// in the space it leaves, blue once letting go does it; let go past the line
// (or flicked) it archives, and a row in Archived goes back to the Inbox
// instead. Dragged any other way with the mouse, it's the thread itself going
// out, to drop into a thread's message as its chip (`drag.threadout`;
// composer_inline.rs takes the drop).
//
// The decisions are core's RowSwipe, so they're tested without a trackpad.
// The trackpad's movements come as scroll events over the row. A mouse press
// is watched from the window in the capture phase, before GPUI's own drag of
// the thread can start: until it's clear which way the mouse is going the
// moves are held from it, and once it's left they stay the row's.
//
// The row moves the way the content scrolls, as a page swipe does, so
// archiving (the row going left) is never the same gesture as Back (content
// going right), whichever way the system scrolls (RowSwipeController.handle).
use crate::app::Threadbear;
use crate::composer_inline::DraggedThread;
use crate::sidebar::{icon, CORNER};
use crate::style::{self, px};
use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use threadbear_core::row_swipe::{Phase, RowSwipe};
use threadbear_core::sidebar::{RowAction, RowMode};

/// A swiped row going the rest of the way off before it archives
/// (`.easeIn(duration: Juice.reduceMotion ? 0.1 : 0.16)`).
const SLIDE_OFF: Duration = Duration::from_millis(160);
const SLIDE_OFF_REDUCED: Duration = Duration::from_millis(100);
/// A row let go short of the line springing back: `.spring(response: 0.3,
/// dampingFraction: 0.75)` has settled by then; Reduce Motion's is a plain
/// `.easeOut(duration: 0.15)`.
const SPRING_BACK: f32 = 0.45;
const SPRING_BACK_REDUCED: f32 = 0.15;
/// Longer than this between the last movement and the lift isn't a flick: a
/// quick move, a pause, then a lift (the page swipe's `flickGap`).
const FLICK_GAP: Duration = Duration::from_millis(100);

/// What letting go of a swiped row does (`RowSwipeAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwipeAction {
    Archive,
    Unarchive,
}

impl SwipeAction {
    pub(crate) fn of(mode: RowMode) -> Option<SwipeAction> {
        if mode.actions().contains(&RowAction::Archive) {
            Some(SwipeAction::Archive)
        } else if mode.actions().contains(&RowAction::Unarchive) {
            Some(SwipeAction::Unarchive)
        } else {
            None
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            SwipeAction::Archive => "archivebox",
            SwipeAction::Unarchive => "arrow.uturn.backward",
        }
    }
}

/// A mouse press on a row, watched to see what it becomes.
struct Press {
    row: String,
    action: Option<SwipeAction>,
    start: Point<Pixels>,
    last: Point<Pixels>,
    /// Where the window was before the press opened the row's thread.
    before: crate::shell::NavPlace,
    decided: Option<Decided>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Decided {
    /// Left: the row's swipe (or nothing, for a row with no swipe).
    Swipe,
    /// Any other way: GPUI's drag of the thread.
    DragOut,
}

/// A row going back into place or off the list: which, what it does, from
/// where and since when.
#[derive(Clone)]
struct Settling {
    row: String,
    action: SwipeAction,
    from: f32,
    at: Instant,
}

/// One row's gesture at a time, for the whole sidebar.
#[derive(Default)]
pub(crate) struct RowGestures {
    swipe: RowSwipe,
    /// The row the swipe is on, and what letting go does to it.
    row: Option<(String, SwipeAction)>,
    press: Option<Press>,
    /// The fingers have touched: macOS says so (MayBegin) before their first
    /// movement (Began), and GPUI calls both Started.
    touched: bool,
    /// After a swipe the trackpad keeps sending its momentum as more moves:
    /// they're the row's, or the list would scroll once the fingers lift.
    eating_momentum: bool,
    /// When the last of that momentum came.
    momentum_at: Option<Instant>,
    spring: Option<Settling>,
    leaving: Option<Settling>,
    /// A row's width, as last drawn: every row's is the list's.
    width: Rc<Cell<f32>>,
    /// The row under the pointer, if it swipes (`RowSwipeController.isHovered`).
    hovered: Option<String>,
    /// When the swipe last moved, for how fast it's going (a flick).
    moved_at: Option<Instant>,
}

impl RowGestures {
    /// The pointer's on a row that swipes: a gesture starting now is the
    /// row's to look at too, so a page swipe doesn't hold it back.
    pub(crate) fn pointer_on_swipe_row(&self) -> bool {
        self.hovered.is_some()
    }

    /// The pointer came onto a row or left it; `action` is what its swipe
    /// does, if it has one.
    pub(crate) fn row_hovered(&mut self, id: &str, hovered: bool, action: Option<SwipeAction>) {
        if hovered && action.is_some() {
            self.hovered = Some(id.to_owned());
        } else if self.hovered.as_deref() == Some(id) {
            self.hovered = None;
        }
    }

    /// Following the fingers, springing back or going: its hover buttons stay
    /// away meanwhile, as they'd ride along (`showsActions`).
    pub(crate) fn is_moving(&self, id: &str) -> bool {
        let tracking = self.swipe.is_swiping() && self.row.as_ref().is_some_and(|(row, _)| row == id);
        tracking || self.leaving.as_ref().is_some_and(|s| s.row == id)
    }

    /// A row's swipe under way, or a row on its way out of the list: an
    /// update's restart waits for it (app_update.rs). Only while it's recent,
    /// so a gesture whose end never came can't hold the update off for good.
    pub(crate) fn under_way(&self) -> bool {
        let recent = |at: Option<Instant>, within: f32| at.is_some_and(|at| at.elapsed().as_secs_f32() < within);
        (self.swipe.is_swiping() && recent(self.moved_at, 30.)) || recent(self.leaving.as_ref().map(|s| s.at), 2.)
    }

    /// For scripts: how far along the row is and whether it's past the line,
    /// as the Swift app's `swipe` says it.
    pub(crate) fn along(&self) -> String {
        format!("offset {} armed {}", self.swipe.offset().round() as i64, self.swipe.armed())
    }

    fn fresh_swipe(&mut self, id: &str, action: SwipeAction, right_taken: bool) {
        self.swipe = RowSwipe::new(f64::from(self.width.get().max(1.)));
        self.swipe.begin(right_taken);
        self.row = Some((id.to_owned(), action));
        self.spring = None;
        self.moved_at = None;
    }

    /// A movement of the swipe, timed, so a flick goes by speed rather than
    /// by how far one event happened to move.
    fn move_swipe(&mut self, dx: f64, dy: f64) -> bool {
        let now = Instant::now();
        let elapsed = self.moved_at.map_or(1. / 60., |at| now.saturating_duration_since(at).as_secs_f64());
        self.moved_at = Some(now);
        self.swipe.move_timed(dx, dy, elapsed)
    }

    /// The lift: a flick only if the fingers were still moving as they went.
    fn end_swipe(&mut self) -> bool {
        let still_moving = self.moved_at.is_some_and(|at| at.elapsed() < FLICK_GAP);
        self.swipe.end_moving(still_moving)
    }
}

/// Momentum comes a frame at a time until it stops, so a longer quiet means
/// it has (the kit's MOMENTUM_GAP): smooth-scrolling mouse drivers send
/// movements with no phase, so never the start that otherwise ends it.
const MOMENTUM_GAP: Duration = Duration::from_millis(250);

/// Whether the momentum last heard at `at` is over by now, noting now as
/// when it was last heard.
pub(crate) fn momentum_over(at: &mut Option<Instant>) -> bool {
    let now = Instant::now();
    let over = at.is_some_and(|at| now.saturating_duration_since(at) > MOMENTUM_GAP);
    *at = Some(now);
    over
}

/// The ease of the slide off: in, quickly.
fn ease_in(t: f32) -> f32 {
    t * t * t
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

/// What's left of a spring back after `t` seconds, from 1 to 0, with the
/// small overshoot past its place SwiftUI's spring has (response 0.3 s,
/// damping 0.75).
fn spring_left(t: f32) -> f32 {
    let omega = std::f32::consts::TAU / 0.3;
    let zeta: f32 = 0.75;
    let root = (1. - zeta * zeta).sqrt();
    (-zeta * omega * t).exp() * ((omega * root * t).cos() + zeta / root * (omega * root * t).sin())
}

impl Threadbear {
    // ---------------------------------------------------------------- drawing

    /// A row in its slot: slid by its swipe, with what letting go will do in
    /// the space it leaves on the right (`RowSwipeModifier`). The backing is
    /// only as wide as the gap, as the rows have no fill of their own and a
    /// full-width one would show through the text.
    pub(crate) fn swipe_slot(&self, id: &str, action: Option<SwipeAction>, row: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let gestures = &self.sidebar.gestures;
        let reduce = cx.reduce_motion();
        let now = Instant::now();
        let width = gestures.width.get();
        let mut animating = false;
        let mut shown: Option<(f32, bool, SwipeAction)> = None;
        if let Some(leaving) = gestures.leaving.as_ref().filter(|s| s.row == id) {
            // Off the row's left edge, and a little more for its shadow
            // (`gone`), with the backing blue all the way.
            let span = if reduce { SLIDE_OFF_REDUCED } else { SLIDE_OFF };
            let t = (now.saturating_duration_since(leaving.at).as_secs_f32() / span.as_secs_f32()).min(1.);
            animating = t < 1.;
            let gone = -(width + 40.);
            shown = Some((leaving.from + (gone - leaving.from) * ease_in(t), true, leaving.action));
        } else if let Some(spring) = gestures.spring.as_ref().filter(|s| s.row == id) {
            let t = now.saturating_duration_since(spring.at).as_secs_f32();
            let span = if reduce { SPRING_BACK_REDUCED } else { SPRING_BACK };
            animating = t < span;
            let left = match (animating, reduce) {
                (false, _) => 0.,
                (true, true) => 1. - ease_out(t / span),
                (true, false) => spring_left(t),
            };
            shown = Some((spring.from * left, false, spring.action));
        } else if let Some((_, action)) = gestures.row.as_ref().filter(|(row, _)| row == id) {
            shown = Some((gestures.swipe.offset() as f32, gestures.swipe.armed(), *action));
        }
        let measured = gestures.width.clone();
        // Every row's width is the list's; a frame at a time while the row
        // springs back or goes.
        let measure = canvas(
            move |bounds, window, _| {
                measured.set(f32::from(bounds.size.width));
                if animating {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let scrolled = id.to_owned();
        let slot = div()
            .relative()
            .w_full()
            .child(measure)
            // Two fingers over the row: on its slot, which stays put while
            // the row slides.
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                if this.row_scrolled(&scrolled, action, event, window, cx) {
                    cx.stop_propagation();
                }
            }));
        let Some((offset, armed, action)) = shown.filter(|(offset, ..)| offset.abs() >= 0.25) else { return slot.child(row).into_any_element() };
        let backing = (offset < 0.).then(|| {
            let fill = if armed { style::accent(cx) } else { style::primary_opacity(cx, 0.07) };
            let ink = if armed { gpui_kit::white() } else { style::secondary_text(cx) };
            // Hidden until there's room for it, and a touch bigger once
            // letting go does it.
            let room = ((-offset - style::zoomed(16.)) / style::zoomed(24.)).clamp(0., 1.);
            let size = if armed && !reduce { 14. * 1.12 } else { 14. };
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(gpui_kit::px(-offset))
                .rounded(px(CORNER))
                .bg(fill)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(action.symbol(), size, ink).opacity(room))
        });
        slot.children(backing).child(div().relative().left(gpui_kit::px(offset)).child(row)).into_any_element()
    }

    // ---------------------------------------------------------------- the trackpad

    /// Two fingers over a row (`RowSwipeController.handle`). Says whether
    /// the event was the row's, so the list doesn't scroll as well.
    pub(crate) fn row_scrolled(&mut self, id: &str, action: Option<SwipeAction>, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A mouse wheel moves in lines and isn't a swipe.
        let ScrollDelta::Pixels(delta) = event.delta else { return false };
        let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        match event.touch_phase {
            TouchPhase::Started => {
                let gestures = &mut self.sidebar.gestures;
                gestures.eating_momentum = false;
                // The first movement after the fingers touched is more of the
                // same gesture, not a new one.
                let same = gestures.touched && gestures.swipe.phase == Phase::Deciding && gestures.row.as_ref().is_some_and(|(row, _)| row == id);
                gestures.touched = true;
                if !same {
                    if gestures.leaving.is_some() {
                        return false;
                    }
                    let Some(action) = action else {
                        gestures.row = None;
                        gestures.swipe.reset();
                        return false;
                    };
                    // A swipe the other way is Back's while there's somewhere to go.
                    let right_taken = self.swipe_takes_back();
                    self.sidebar.gestures.fresh_swipe(id, action, right_taken);
                }
                self.row_moved(id, dx, dy, cx)
            }
            TouchPhase::Moved => {
                self.sidebar.gestures.touched = false;
                // Momentum after a swipe: nothing else's to scroll with.
                if self.sidebar.gestures.eating_momentum {
                    return true;
                }
                self.row_moved(id, dx, dy, cx)
            }
            TouchPhase::Ended => {
                self.sidebar.gestures.touched = false;
                self.swipe_let_go(id, window, cx)
            }
            // The system took it (macOS sends its cancel as a movement, so
            // this is the other platforms'): never archives.
            TouchPhase::Cancelled => {
                let gestures = &mut self.sidebar.gestures;
                gestures.touched = false;
                let Some((row, action)) = gestures.row.clone().filter(|(row, _)| row == id) else { return false };
                let (was, at) = (gestures.swipe.is_swiping(), gestures.swipe.offset() as f32);
                gestures.swipe.cancel();
                self.spring_back(&row, action, at, cx);
                was
            }
        }
    }

    /// A scroll event seen from the window before anything else: a row's
    /// swipe under way has the rest of the gesture wherever the pointer is
    /// (the row slides out from under it), and the trackpad's momentum after
    /// it, as RowSwipeController's monitor does. None when there's no swipe
    /// under way, for the page swipe and the row under the pointer to look at.
    pub(crate) fn row_swipe_scrolled(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) -> Option<bool> {
        let gestures = &mut self.sidebar.gestures;
        // A mouse wheel's lines are never momentum: they scroll as ever.
        let trackpad = matches!(event.delta, ScrollDelta::Pixels(_));
        if gestures.eating_momentum && trackpad && event.touch_phase == TouchPhase::Moved && !momentum_over(&mut gestures.momentum_at) {
            return Some(true);
        }
        // Fingers back on the trackpad, or a quiet moment: the momentum's over.
        gestures.eating_momentum = false;
        let (id, action) = gestures.row.clone().filter(|_| gestures.swipe.is_swiping())?;
        Some(self.row_scrolled(&id, Some(action), event, window, cx))
    }

    /// The fingers moved over the row.
    fn row_moved(&mut self, id: &str, dx: f64, dy: f64, cx: &mut Context<Self>) -> bool {
        let gestures = &mut self.sidebar.gestures;
        if gestures.row.as_ref().is_none_or(|(row, _)| row != id) || (dx == 0. && dy == 0.) {
            return false;
        }
        let deciding = gestures.swipe.phase == Phase::Deciding;
        let was_armed = gestures.swipe.armed();
        let mine = gestures.move_swipe(dx, dy);
        if mine || gestures.swipe.armed() != was_armed {
            cx.notify();
        }
        // While it's deciding, a movement straight across is kept from the
        // list too: GPUI scrolls a list that only goes up and down by one
        // that only goes across, which jiggled it as a swipe began.
        mine || (deciding && dy == 0.)
    }

    /// The fingers lifted, or the mouse came up: archive past the line (or
    /// on a flick), else spring back. Says whether it was a swipe.
    fn swipe_let_go(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((row, action)) = self.sidebar.gestures.row.clone().filter(|(row, _)| row == id) else { return false };
        let gestures = &mut self.sidebar.gestures;
        let was = gestures.swipe.is_swiping();
        let at = gestures.swipe.offset() as f32;
        let archive = gestures.end_swipe();
        gestures.eating_momentum = was;
        gestures.momentum_at = Some(Instant::now());
        if archive {
            self.finish_swipe(row, action, at, window, cx);
        } else if was {
            self.spring_back(&row, action, at, cx);
        } else {
            // A tap's worth, or a scroll: nothing moved.
            gestures.row = None;
            gestures.swipe.reset();
        }
        was
    }

    /// The gesture's lift won't come here (the window went to the back, and
    /// it goes to whichever app is in front): a row part way along goes back
    /// into place and never archives (`cancelGesture`).
    pub(crate) fn cancel_row_swipe(&mut self, cx: &mut Context<Self>) {
        let gestures = &mut self.sidebar.gestures;
        gestures.touched = false;
        gestures.eating_momentum = false;
        let Some((row, action)) = gestures.row.clone() else { return };
        let (was, at) = (gestures.swipe.is_swiping(), gestures.swipe.offset() as f32);
        gestures.swipe.cancel();
        if was {
            self.spring_back(&row, action, at, cx);
        }
    }

    /// Back into place from where it was let go.
    fn spring_back(&mut self, id: &str, action: SwipeAction, from: f32, cx: &mut Context<Self>) {
        let gestures = &mut self.sidebar.gestures;
        gestures.swipe.reset();
        gestures.row = None;
        gestures.spring = (from.abs() >= 0.5).then(|| Settling { row: id.to_owned(), action, from, at: Instant::now() });
        cx.notify();
    }

    /// Past the line: a busy thread asks first, held where it was let go
    /// (`BusyConfirm`); then the row slides the rest of the way off and the
    /// thread goes.
    fn finish_swipe(&mut self, id: String, action: SwipeAction, at: f32, window: &mut Window, cx: &mut Context<Self>) {
        let busy = action == SwipeAction::Archive && self.threads.get(&id).is_some_and(threadbear_core::sidebar::needs_busy_confirm);
        let Some(thread) = self.threads.get(&id).cloned().filter(|_| busy) else {
            self.slide_off(id, action, at, cx);
            return;
        };
        // Held where it was let go while it asks: still the swipe's row.
        self.sidebar.gestures.row = Some((id.clone(), action));
        let (title, detail) = threadbear_core::sidebar::busy_prompt(&thread, true);
        let (ok_app, close_app) = (cx.entity().downgrade(), cx.entity().downgrade());
        let (ok_id, close_id) = (id.clone(), id);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ok_app, close_app, ok_id, close_id) = (ok_app.clone(), close_app.clone(), ok_id.clone(), close_id.clone());
            alert
                .title(title.clone())
                .description(detail.clone())
                .button_props(gpui_kit::component::dialog::DialogButtonProps::default().ok_text("Archive").show_cancel(true))
                .on_ok(move |_, _, cx| {
                    ok_app.update(cx, |this, cx| this.slide_off(ok_id.clone(), action, at, cx)).ok();
                    true
                })
                // Cancel, Escape or a click outside: back into place, unless
                // it's already on its way off.
                .on_close(move |_, _, cx| {
                    close_app
                        .update(cx, |this, cx| {
                            if this.sidebar.gestures.row.as_ref().is_some_and(|(row, _)| *row == close_id) {
                                this.spring_back(&close_id, action, at, cx);
                            }
                        })
                        .ok();
                })
        });
    }

    /// Off to the left with the backing blue behind it, then archived (with
    /// the Archived note a swipe's archive has, `slid: true`) or put back in
    /// the Inbox.
    fn slide_off(&mut self, id: String, action: SwipeAction, at: f32, cx: &mut Context<Self>) {
        let quick = if cx.reduce_motion() { SLIDE_OFF_REDUCED } else { SLIDE_OFF };
        let gestures = &mut self.sidebar.gestures;
        gestures.swipe.reset();
        gestures.row = None;
        gestures.leaving = Some(Settling { row: id.clone(), action, from: at, at: Instant::now() });
        cx.notify();
        let (me, window) = (cx.entity().downgrade(), self.main_window);
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(quick).await;
            let id2 = id.clone();
            window
                .update(cx, |_, window, cx| {
                    me.update(cx, |this, cx| match action {
                        SwipeAction::Archive => this.archive_row(id2, window, cx),
                        SwipeAction::Unarchive => this.unarchive_row(&id2, cx),
                    })
                    .ok();
                })
                .ok();
            // Normally the row's gone by now. One still drawn (the archive
            // was refused, or a list that keeps archived rows) comes back.
            cx.background_executor().timer(Duration::from_millis(600)).await;
            me.update(cx, |this, cx| {
                if this.sidebar.gestures.leaving.as_ref().is_some_and(|s| s.row == id) {
                    this.sidebar.gestures.leaving = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // ---------------------------------------------------------------- the mouse

    /// The mouse went down on a row, which opens its thread: watched from
    /// here to see whether it's a click, a drag left (the swipe) or the
    /// thread being dragged out. `before` is where the window was.
    pub(crate) fn row_pressed(&mut self, id: &str, action: Option<SwipeAction>, at: Point<Pixels>, before: crate::shell::NavPlace) {
        self.sidebar.gestures.press = Some(Press { row: id.to_owned(), action, start: at, last: at, before, decided: None });
    }

    /// The mouse moved with the button down, seen before anything else in the
    /// window. Says whether to keep it from the rest: while it's not yet
    /// clear, and all through a swipe, so GPUI doesn't start dragging the
    /// thread. Once it's the thread being dragged out, the press opened that
    /// thread, but it's being dragged into the thread that was open, so that
    /// one comes back to drop it on (`dragOut`).
    pub(crate) fn row_press_moved(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(press) = self.sidebar.gestures.press.as_mut() else { return false };
        let (dx, dy) = (f64::from(f32::from(at.x - press.start.x)), f64::from(f32::from(at.y - press.start.y)));
        let (step_x, step_y) = (f64::from(f32::from(at.x - press.last.x)), f64::from(f32::from(at.y - press.last.y)));
        press.last = at;
        match press.decided {
            Some(Decided::DragOut) => false,
            Some(Decided::Swipe) => {
                let row = press.row.clone();
                let gestures = &mut self.sidebar.gestures;
                if gestures.row.as_ref().is_some_and(|(r, _)| *r == row) {
                    gestures.move_swipe(step_x, step_y);
                    cx.notify();
                }
                true
            }
            None if RowSwipe::drags_out(dx, dy) => {
                press.decided = Some(Decided::DragOut);
                let before = press.before.clone();
                self.sidebar.gestures.row = None;
                self.sidebar.gestures.swipe.reset();
                if self.place() != before {
                    self.go_back(window, cx);
                }
                cx.notify();
                false
            }
            None => {
                let (row, action) = (press.row.clone(), press.action);
                let gestures = &mut self.sidebar.gestures;
                if let Some(action) = action.filter(|_| gestures.leaving.is_none()) {
                    if gestures.row.as_ref().is_none_or(|(r, _)| *r != row) || gestures.swipe.phase == Phase::Idle {
                        gestures.fresh_swipe(&row, action, false);
                    }
                    gestures.move_swipe(step_x, step_y);
                }
                // Left, by the same rule a swipe goes by: the row's, whether or
                // not it has a swipe (a hibernating row has none).
                if dx.abs().max(dy.abs()) >= threadbear_core::row_swipe::DECIDE_AFTER {
                    if let Some(press) = gestures.press.as_mut() {
                        press.decided = Some(Decided::Swipe);
                    }
                }
                cx.notify();
                true
            }
        }
    }

    /// The mouse button came up after a press on a row.
    pub(crate) fn row_press_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(press) = self.sidebar.gestures.press.take() else { return };
        if press.decided == Some(Decided::DragOut) {
            return;
        }
        if self.sidebar.gestures.swipe.is_swiping() {
            self.swipe_let_go(&press.row, window, cx);
        } else if self.sidebar.gestures.row.as_ref().is_some_and(|(row, _)| *row == press.row) {
            // A plain click never left deciding: nothing to spring back.
            self.sidebar.gestures.swipe.reset();
            self.sidebar.gestures.row = None;
        }
    }

    /// The window's watch on a press, one for the whole list: the moves and
    /// the release, in the capture phase (`ThreadRowView`'s DragGesture).
    pub(crate) fn row_press_watch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let app = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let (moved, released) = (app.clone(), app.clone());
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.pressed_button != Some(MouseButton::Left) {
                        return;
                    }
                    if moved.update(cx, |this, cx| this.row_press_moved(event.position, window, cx)).unwrap_or(false) {
                        cx.stop_propagation();
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        released.update(cx, |this, cx| this.row_press_released(window, cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The thread itself, for GPUI's drag of a row out.
    pub(crate) fn dragged_thread(&self, id: &str) -> DraggedThread {
        DraggedThread { id: id.to_owned(), title: self.threads.get(id).map(|t| t.title.clone()).unwrap_or_default() }
    }
}

/// What follows the pointer while a row's dragged out: the thread's chip as
/// it'll be in the message (composer_inline.rs draws it), held by its left
/// end a little way in, as ThreadDrag holds it. GPUI draws a drag where the
/// row was grabbed, so the chip's moved back under the pointer.
pub(crate) struct RowDragChip {
    chip: Entity<DraggedThread>,
    grabbed: Point<Pixels>,
}

impl RowDragChip {
    pub(crate) fn new(drag: &DraggedThread, grabbed: Point<Pixels>, cx: &mut App) -> Entity<Self> {
        let chip = cx.new(|_| drag.clone());
        cx.new(|_| RowDragChip { chip, grabbed })
    }
}

impl Render for RowDragChip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let left = (self.grabbed.x - px(14.)).max(gpui_kit::px(0.));
        let top = (self.grabbed.y - px(style::size::CHIP_HEIGHT / 2.)).max(gpui_kit::px(0.));
        div().pl(left).pt(top).child(self.chip.clone())
    }
}

// ---------------------------------------------------------------- scripts

/// The row's middle on screen, in the window's points, found by its tooltip
/// (its title, then its folder), as a person sees it.
#[cfg(feature = "automation")]
fn row_middle(app: &Entity<Threadbear>, id: &str, window: &mut Window, cx: &mut App) -> Result<Point<Pixels>, String> {
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    window.render_frame(cx);
    let title = app.read(cx).threads.get(id).map(|t| if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() }).ok_or_else(|| format!("no thread {id}"))?;
    let label = format!("{title}\n");
    let row = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| l.starts_with(&label))).last();
    row.map(|r| r.bounds().center()).ok_or_else(|| format!("the row {title:?} isn't on screen"))
}

/// The row's middle once it's on screen and has stopped moving: a section
/// folding or unfolding eases its rows shut or open over a moment, and the
/// rows below it slide meanwhile.
#[cfg(feature = "automation")]
async fn row_on_screen(app: &Entity<Threadbear>, id: &str, window: AnyWindowHandle, cx: &mut AsyncApp) -> Result<Point<Pixels>, String> {
    let mut last = None;
    for _ in 0..60 {
        let found = cx.update_window(window, |_, window, cx| row_middle(app, id, window, cx)).map_err(|e| e.to_string())?;
        match found {
            Ok(at) if last == Some(at) => return Ok(at),
            Ok(at) => last = Some(at),
            Err(_) => last = None,
        }
        cx.background_executor().timer(Duration::from_millis(60)).await;
    }
    last.ok_or_else(|| "that row isn't on screen".to_owned())
}

/// `row-point`: where a row's middle is, "ok x y", for a real event there
/// (features/tools/real-gestures.ts).
#[cfg(feature = "automation")]
pub(crate) fn row_point_command(app: &Entity<Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    let id = app.read(cx).thread_id_from(cmd["id"].as_str().unwrap_or(""))?;
    let at = row_middle(app, &id, window, cx)?;
    Ok(serde_json::json!(format!("ok {} {}", f32::from(at.x).round(), f32::from(at.y).round())))
}

/// `swipe`, as the Swift app's: two fingers across a sidebar row
/// (`{"id": thread, "dx": -150}`, the content's way, left negative), as
/// trackpad scrolls through the window's own dispatch, the pointer moved
/// over the row first. `"lift": false` leaves the fingers down, for a
/// picture of the row part way along; a later swipe (with any `dx`, 0
/// included) carries on and lets go. Says where the row got to and whether
/// it's past the line, then whether letting go archived it.
#[cfg(feature = "automation")]
pub(crate) async fn swipe_command(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    let app = app.upgrade().ok_or("the app has gone")?;
    let id = app.read_with(cx, |t, _| t.thread_id_from(cmd["id"].as_str().unwrap_or("")))?;
    let dx = cmd["dx"].as_f64().unwrap_or(0.);
    let steps = cmd["steps"].as_u64().unwrap_or(8).max(1);
    let at = row_on_screen(&app, &id, window, cx).await?;
    cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_move(at, None, cx))).map_err(|e| e.to_string())?;
    let send = |x: f64, phase: TouchPhase, cx: &mut AsyncApp| {
        cx.update_window(window, |_, window, cx| {
            let event = ScrollEvent { position: at, delta: ScrollDelta::Pixels(point(gpui_kit::px(x as f32), gpui_kit::px(0.))), modifiers: Modifiers::default(), touch_phase: phase, source: ScrollSource::Pointer };
            crate::thread::scripted_input(|| {
                window.dispatch_event(event.to_platform_input(), cx);
                window.render_frame(cx);
            });
        })
        .map_err(|e| e.to_string())
    };
    let fresh = app.read_with(cx, |t, _| t.sidebar.gestures.row.is_none());
    if fresh {
        send(0., TouchPhase::Started, cx)?;
    }
    for _ in 0..steps {
        send(dx / steps as f64, TouchPhase::Moved, cx)?;
        cx.background_executor().timer(Duration::from_millis(16)).await;
    }
    let along = app.read_with(cx, |t, _| t.sidebar.gestures.along());
    if cmd["lift"].as_bool() == Some(false) {
        return Ok(serde_json::json!(format!("ok {along}")));
    }
    send(0., TouchPhase::Ended, cx)?;
    cx.background_executor().timer(Duration::from_millis(500)).await;
    let archived = app.read_with(cx, |t, _| t.threads.get(&id).is_some_and(|t| t.archived_at.is_some()));
    Ok(serde_json::json!(format!("ok {along}, let go: {}", if archived { "archived" } else { "not archived" })))
}

/// `row-drag`: the mouse pressed on a row and dragged, by `dx` and `dy` or
/// to the window's point `x`, `y`, then let go, through the window's own
/// dispatch: left is the swipe, any other way the thread dragged out. Says
/// how far the row got, then whether letting go archived it.
#[cfg(feature = "automation")]
pub(crate) async fn row_drag_command(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::TestEventExt as _;
    let app = app.upgrade().ok_or("the app has gone")?;
    let id = app.read_with(cx, |t, _| t.thread_id_from(cmd["id"].as_str().unwrap_or("")))?;
    let from = row_on_screen(&app, &id, window, cx).await?;
    cx.update_window(window, |_, window, cx| {
        crate::thread::scripted_input(|| {
            window.pointer_move(from, None, cx);
            window.pointer_down(from, MouseButton::Left, cx);
        })
    })
    .map_err(|e| e.to_string())?;
    let to = match (cmd["x"].as_f64(), cmd["y"].as_f64()) {
        (Some(x), Some(y)) => point(gpui_kit::px(x as f32), gpui_kit::px(y as f32)),
        _ => point(from.x + gpui_kit::px(cmd["dx"].as_f64().unwrap_or(0.) as f32), from.y + gpui_kit::px(cmd["dy"].as_f64().unwrap_or(0.) as f32)),
    };
    let steps = 12;
    let mut along = String::new();
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let at = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_move(at, Some(MouseButton::Left), cx))).map_err(|e| e.to_string())?;
        cx.background_executor().timer(Duration::from_millis(16)).await;
        if step == steps {
            along = app.read_with(cx, |t, _| t.sidebar.gestures.along());
        }
    }
    cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| window.pointer_up(to, MouseButton::Left, cx))).map_err(|e| e.to_string())?;
    cx.background_executor().timer(Duration::from_millis(500)).await;
    let archived = app.read_with(cx, |t, _| t.threads.get(&id).is_some_and(|t| t.archived_at.is_some()));
    Ok(serde_json::json!(format!("ok {along}, let go: {}", if archived { "archived" } else { "not archived" })))
}
