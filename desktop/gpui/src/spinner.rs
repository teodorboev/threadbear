// The turning arc a working thread shows in the sidebar (Spinner.swift's
// SpinnerView): three quarters of a ring with round ends, once round every
// 0.9 s, clockwise.
//
// The Swift app turns it in the render server, with nothing for the main
// thread to do. GPUI turns a thing by drawing it again, and a request for the
// next frame from inside a view has that view and every view it's in (the
// sidebar, the dock, the window, the open transcript) draw again. The kit's
// spinner asks every frame, so while any thread worked the whole window drew
// at the screen's rate, 120 times a second on Mike's MacBook, and a debug
// build spent all its time there. This one waits 20 ms after it's drawn
// before asking again, one timer for all the arcs a view has: with the wait
// for the screen's next frame on top, about 30 frames a second, which reads
// as smooth at this size. (33 ms came to 20 a second, a visible step.)
use crate::style::{self, px};
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const TURN_SECONDS: f32 = 0.9;
const FRAME: Duration = Duration::from_millis(20);

#[derive(IntoElement)]
pub(crate) struct TurningArc {
    side: f32,
    color: Hsla,
}

impl TurningArc {
    /// `side` in points, before the zoom.
    pub(crate) fn new(side: f32, color: Hsla) -> Self {
        Self { side, color }
    }
}

impl RenderOnce for TurningArc {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self.color;
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                paint_arc(bounds, color, window);
                next_frame(window, cx);
            },
        )
        .flex_none()
        .size(px(self.side))
    }
}

fn paint_arc(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    static START: OnceLock<Instant> = OnceLock::new();
    let turned = (START.get_or_init(Instant::now).elapsed().as_secs_f32() / TURN_SECONDS).fract();
    // The ring as Swift's: 1.6 wide, 3 in from the sides, zoomed like an
    // icon's strokes.
    let width = style::zoomed(1.6);
    let side = f32::from(bounds.size.width).min(f32::from(bounds.size.height));
    let radius = ((side - style::zoomed(3.)) / 2.).max(1.);
    let center = bounds.center();
    let at = |angle: f32| point(center.x + gpui_kit::px(radius * angle.cos()), center.y + gpui_kit::px(radius * angle.sin()));
    let start = turned * std::f32::consts::TAU;
    let end = start + 0.75 * std::f32::consts::TAU;
    let mut path = PathBuilder::stroke(gpui_kit::px(width));
    path.move_to(at(start));
    // Clockwise on screen (y runs down), the long way round.
    path.arc_to(point(gpui_kit::px(radius), gpui_kit::px(radius)), gpui_kit::px(0.), true, true, at(end));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
    // Round ends: lyon's caps aren't GPUI's to set, so a dot at each.
    for angle in [start, end] {
        let c = at(angle);
        let dot = Bounds { origin: point(c.x - gpui_kit::px(width / 2.), c.y - gpui_kit::px(width / 2.)), size: size(gpui_kit::px(width), gpui_kit::px(width)) };
        window.paint_quad(fill(dot, color).corner_radii(gpui_kit::px(width / 2.)));
    }
}

thread_local! {
    /// The views with a frame on its way for their arcs.
    static TICKING: RefCell<HashSet<EntityId>> = RefCell::new(HashSet::new());
}

/// The view this arc is in draws again in a moment, once for all its arcs.
fn next_frame(window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    if !TICKING.with(|t| t.borrow_mut().insert(view)) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FRAME).await;
        TICKING.with(|t| t.borrow_mut().remove(&view));
        let _ = cx.update(|cx| cx.notify(view));
    })
    .detach();
}
