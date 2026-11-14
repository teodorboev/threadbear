// Trackpad swipes going back and forward, as in Safari (the Swift app's
// SwipeNavigation): two fingers across the window, when System Settings has
// "Swipe between pages" on for scrolling. A card in the middle of the window
// says where letting go will go, filling from the side the fingers start on
// and turning blue at the line (`SwipeNavigationArrow`, the card Mike picked
// on 8 Nov); let go past it, or flicked, goes there. Three-finger swipes,
// which macOS sends as swipe events rather than scrolls, don't reach GPUI.
//
// A gesture is held back from whatever's under the pointer until it's clear
// which way it's going, a few points, and handed on to it if it's a scroll,
// as the Swift app (and WebKit) do. Over a sidebar row nothing's held: the
// row needs the whole gesture to slide, and gives a swipe the other way up to
// Back while there's somewhere to go (`swipe_takes_back`). Back moves the
// content right, whichever way the system scrolls, as GPUI's deltas are the
// content's way.
//
// The decisions are core's NavigationSwipe, so they're tested without a
// trackpad. The events are watched from the window, in the capture phase,
// before anything that scrolls gets them.
use crate::app::Threadbear;
use crate::style::{self, px};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Instant;
use threadbear_core::navigation_swipe::{Direction, NavigationSwipe, Phase, Verdict};

/// Longer than this between the last movement and the lift isn't a flick
/// (`flickGap`): a quick move, a pause to look at the card, then a lift.
const FLICK_GAP: f64 = 0.1;

/// The window's page swipe; the app holds one.
#[derive(Default)]
pub(crate) struct SwipeNav {
    pub(crate) swipe: NavigationSwipe,
    /// Natural scrolling, for the gesture under way: which side the card
    /// fills from.
    natural: bool,
    /// The fingers have touched: macOS says so (MayBegin) before their first
    /// movement (Began), and GPUI calls both Started.
    touched: bool,
    /// After a page swipe the trackpad keeps sending its momentum as more
    /// moves: they're the swipe's, or whatever's under the pointer scrolls.
    eating_momentum: bool,
    /// When the last of that momentum came (row_swipe.rs, `momentum_over`).
    momentum_at: Option<Instant>,
    /// When the last of this gesture's events came, for its speed.
    last_event: Option<Instant>,
    /// The events held back while deciding, to hand on if it's a scroll.
    held: Vec<ScrollEvent>,
    /// Handing those on: they're on their way, not new.
    replaying: bool,
    /// A gesture begun over a board belongs to its scrollers through lift.
    board_scroll: bool,
}

impl SwipeNav {
    /// A page swipe following the fingers: an update's restart waits for it
    /// (app_update.rs). Only while its events are recent, so a gesture whose
    /// end never came can't hold the update off for good.
    pub(crate) fn under_way(&self) -> bool {
        (self.touched || self.swipe.direction().is_some()) && self.last_event.is_some_and(|at| at.elapsed().as_secs_f32() < 30.)
    }
}

/// System Settings' "Swipe between pages" with two fingers
/// (`NSEvent.isSwipeTrackingFromScrollEventsEnabled`). Elsewhere it's on.
pub(crate) fn scroll_swipes_on() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[allow(unused_unsafe)]
        unsafe {
            objc2_app_kit::NSEvent::isSwipeTrackingFromScrollEventsEnabled()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Natural scrolling, as System Settings has it (the global
/// `com.apple.swipescrolldirection`, on unless it's been turned off). GPUI's
/// events don't carry `isDirectionInvertedFromDevice`.
fn natural_scrolling() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSString, NSUserDefaults};
        let key = NSString::from_str("com.apple.swipescrolldirection");
        #[allow(unused_unsafe)]
        unsafe {
            let defaults = NSUserDefaults::standardUserDefaults();
            defaults.objectForKey(&key).is_none() || defaults.boolForKey(&key)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

impl Threadbear {
    /// Whether a swipe moving the content right goes Back just now: a row
    /// deciding what a swipe is asks, so the two never both take it.
    pub(crate) fn swipe_takes_back(&self) -> bool {
        scroll_swipes_on() && self.can_go_back()
    }

    /// The ways there's somewhere to go.
    fn history_ways(&self) -> Vec<Direction> {
        let mut ways = Vec::new();
        if self.can_go_back() {
            ways.push(Direction::Back);
        }
        if self.can_go_forward() {
            ways.push(Direction::Forward);
        }
        ways
    }

    /// A scroll event, seen before anything else in the window. Says whether
    /// it's the swipe's, to be kept from the rest. `over_row`: the pointer's
    /// on a sidebar row that swipes, which needs the gesture too.
    pub(crate) fn page_scrolled(&mut self, event: &ScrollEvent, over_row: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A mouse wheel moves in lines and has no phases: never a swipe.
        let ScrollDelta::Pixels(delta) = event.delta else { return false };
        let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        let now = Instant::now();
        let elapsed = self.swipe_nav.last_event.map_or(1. / 60., |at| now.saturating_duration_since(at).as_secs_f64());
        match event.touch_phase {
            TouchPhase::Started => {
                let nav = &mut self.swipe_nav;
                nav.eating_momentum = false;
                nav.last_event = Some(now);
                // The first movement after the fingers touched is more of the
                // same gesture.
                let same = nav.touched && nav.swipe.phase == Phase::Deciding;
                nav.touched = true;
                if !same {
                    if !scroll_swipes_on() {
                        self.swipe_nav.swipe.reset();
                        return false;
                    }
                    self.swipe_nav.natural = natural_scrolling();
                    let ways = self.history_ways();
                    // Over a row, content going left is the row's (archive)
                    // and a way with nowhere to go is too (it gives a little
                    // and stops); elsewhere the card says there's nowhere to go.
                    let (allowed, empty): (Vec<Direction>, Vec<Direction>) = if over_row {
                        (ways.into_iter().filter(|w| *w == Direction::Back).collect(), Vec::new())
                    } else {
                        let empty = [Direction::Back, Direction::Forward].into_iter().filter(|w| !ways.contains(w)).collect();
                        (ways, empty)
                    };
                    self.swipe_nav.swipe.begin(&allowed, &empty, !over_row);
                }
                if dx == 0. && dy == 0. {
                    // The fingers touching is held too, while it's the
                    // window's to decide.
                    let holds = self.swipe_nav.swipe.holds && self.swipe_nav.swipe.phase == Phase::Deciding;
                    if holds {
                        self.swipe_nav.held.push(event.clone());
                    }
                    return holds;
                }
                self.page_moved(event, dx, dy, elapsed, window, cx)
            }
            TouchPhase::Moved => {
                self.swipe_nav.touched = false;
                // Momentum after a page swipe: nobody else's, till it stops.
                if self.swipe_nav.eating_momentum && !crate::row_swipe::momentum_over(&mut self.swipe_nav.momentum_at) {
                    return true;
                }
                self.swipe_nav.eating_momentum = false;
                self.swipe_nav.last_event = Some(now);
                self.page_moved(event, dx, dy, elapsed, window, cx)
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let nav = &mut self.swipe_nav;
                nav.touched = false;
                let tracking = nav.swipe.direction().is_some();
                let holds = nav.swipe.holds;
                let to = if event.touch_phase == TouchPhase::Ended {
                    nav.swipe.end(elapsed < FLICK_GAP)
                } else {
                    nav.swipe.reset();
                    None
                };
                nav.eating_momentum = tracking;
                nav.momentum_at = Some(now);
                nav.last_event = None;
                // Lifted before it was clear: a tap's worth, and what was
                // held is the view's to have, the lift after it.
                if !tracking && !nav.held.is_empty() {
                    self.release_held(event, window, cx);
                    return true;
                }
                nav.held.clear();
                match to {
                    Some(Direction::Back) => self.go_back(window, cx),
                    Some(Direction::Forward) => self.go_forward(window, cx),
                    None => {}
                }
                if tracking {
                    cx.notify();
                }
                // Over a row the row saw the gesture start, and needs its
                // end to stop listening.
                tracking && holds
            }
        }
    }

    fn page_moved(&mut self, event: &ScrollEvent, dx: f64, dy: f64, elapsed: f64, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let swipe = &mut self.swipe_nav.swipe;
        let (was, was_armed) = (swipe.direction(), swipe.armed());
        let verdict = swipe.move_by(dx, dy, elapsed);
        if swipe.direction().is_some() || was.is_some() || swipe.armed() != was_armed {
            cx.notify();
        }
        match verdict {
            Verdict::Wait if swipe.holds => {
                self.swipe_nav.held.push(event.clone());
                true
            }
            Verdict::Wait => false,
            // A scroll after all: what was held goes on to whatever's under
            // the pointer, and this after it.
            Verdict::Scroll if !self.swipe_nav.held.is_empty() => {
                self.release_held(event, window, cx);
                true
            }
            Verdict::Scroll => false,
            Verdict::Swipe => {
                self.swipe_nav.held.clear();
                true
            }
        }
    }

    /// Hands the held events on, in order, then `last`, once this one's been
    /// dispatched: from inside it they'd find nobody listening.
    fn release_held(&mut self, last: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mut events = std::mem::take(&mut self.swipe_nav.held);
        events.push(last.clone());
        let me = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            me.update(cx, |this, _| this.swipe_nav.replaying = true).ok();
            for event in events {
                window.dispatch_event(event.to_platform_input(), cx);
            }
            me.update(cx, |this, _| this.swipe_nav.replaying = false).ok();
        });
    }

    /// The gesture's lift won't come here (the window went to the back).
    pub(crate) fn cancel_page_swipe(&mut self, cx: &mut Context<Self>) {
        let nav = &mut self.swipe_nav;
        if nav.swipe.phase == Phase::Idle && !nav.touched && !nav.eating_momentum {
            return;
        }
        nav.swipe.reset();
        nav.touched = false;
        nav.eating_momentum = false;
        nav.last_event = None;
        nav.held.clear();
        cx.notify();
    }

    /// The window's watch on scroll events, in the capture phase, so it hears
    /// them before anything that scrolls.
    pub(crate) fn page_swipe_watch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let app = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let app = app.clone();
                window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    // Held events handed on: they go straight through. And
                    // nothing goes back or forward behind a dialog, which
                    // has the window (the Swift app's are windows of their own).
                    if app.upgrade().is_none_or(|this| this.read(cx).swipe_nav.replaying) || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
                        return;
                    }
                    let mine = app
                        .update(cx, |this, cx| {
                            if event.touch_phase == TouchPhase::Started {
                                this.swipe_nav.board_scroll = this.panel_available(crate::shell::HIGHLIGHTS, cx)
                                    && this.dock.read(cx).controller.is_showing(crate::shell::HIGHLIGHTS)
                                    && this.pane_thread().is_some_and(|thread| this.side.read(cx).board_scroll_at(&thread, event.position, window));
                                if this.swipe_nav.board_scroll {
                                    this.swipe_nav.swipe.reset();
                                    this.swipe_nav.held.clear();
                                    this.swipe_nav.touched = false;
                                    this.swipe_nav.eating_momentum = false;
                                }
                            }
                            if this.swipe_nav.board_scroll {
                                if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                                    this.swipe_nav.board_scroll = false;
                                }
                                return false;
                            }
                            // A row's swipe under way has the whole gesture,
                            // wherever the pointer's got to (row_swipe.rs).
                            if let Some(mine) = this.row_swipe_scrolled(event, window, cx) {
                                return mine;
                            }
                            let over_row = this.sidebar.gestures.pointer_on_swipe_row();
                            this.page_scrolled(event, over_row, window, cx)
                        })
                        .unwrap_or(false);
                    if mine {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The card in the middle of the window while a page swipe is under way:
    /// which thread or page letting go will open (`SwipeNavigationArrow`).
    pub(crate) fn render_swipe_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let swipe = &self.swipe_nav.swipe;
        let direction = swipe.direction()?;
        let back = direction == Direction::Back;
        let place = self.destination(back);
        // Nowhere to go: said, rather than nothing showing at all.
        let empty = swipe.is_empty() || place.is_none();
        let title = place.unwrap_or_else(|| if back { "Nothing to go back to".into() } else { "Nothing to go forward to".into() });
        let detail = if empty { None } else { self.destination_detail(back) };
        let armed = swipe.armed();
        let progress = swipe.progress() as f32;
        let accent = style::accent(cx);
        let (secondary, fg) = (style::secondary_text(cx), cx.theme().foreground);
        let width = style::zoomed(340.);
        // The fill starts from the side the fingers start on: Back moves the
        // content right, the fingers that way too with natural scrolling,
        // the other way without.
        let fingers_left = back != self.swipe_nav.natural;
        let fill = div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(gpui_kit::px(width * progress))
            .when(fingers_left, |s| s.right_0())
            .when(!fingers_left, |s| s.left_0())
            .bg(if armed { accent.opacity(0.32) } else { style::primary_opacity(cx, 0.07) });
        let circle = div()
            .flex_none()
            .size(px(34.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if armed { accent } else { style::primary_opacity(cx, 0.08) })
            .child(crate::sidebar::icon(if back { "chevron.left" } else { "chevron.right" }, 14., if armed { gpui_kit::white() } else { secondary }));
        let label = style::size::LABEL;
        let words = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .when(!empty, |s| s.child(div().text_size(px(label)).line_height(px(style::natural_line(label))).text_color(secondary).child(if back { "Back to" } else { "Forward to" })))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(style::natural_line(14.)))
                    .line_clamp(2)
                    .overflow_hidden()
                    .when(empty, |s| s.text_color(secondary))
                    .when(!empty, |s| s.font_weight(FontWeight::SEMIBOLD).text_color(fg))
                    .child(title),
            )
            .when_some(detail, |s, d| s.child(div().truncate().text_size(px(label)).line_height(px(style::natural_line(label))).text_color(secondary).child(d)));
        let card = div()
            .relative()
            .w(gpui_kit::px(width))
            .rounded(px(14.))
            .overflow_hidden()
            // Solid: the transcript mustn't show through behind the name.
            .bg(cx.theme().background)
            .border(if armed { px(1.) } else { px(0.5) })
            .border_color(if armed { accent } else { style::hairline(cx) })
            .shadow(vec![BoxShadow::new(gpui_kit::px(0.), px(6.), hsla(0., 0., 0., 0.4)).blur_radius(px(20.))])
            .child(fill)
            .child(h_flex().relative().px(px(18.)).py(px(16.)).gap(px(12.)).items_center().child(circle).child(words));
        // In as soon as it's clear it's a swipe, not with the line; one with
        // nowhere to go only gives a little, so it's solid at once.
        let opacity = if swipe.is_empty() { 1. } else { (0.4 + progress * 2.).min(1.) };
        Some(div().absolute().inset_0().flex().items_center().justify_center().opacity(opacity).child(card).into_any_element())
    }

    /// The card's second line: a thread's place, as its sidebar row says it
    /// (`describe`), or "Project" for a project's page.
    fn destination_detail(&self, back: bool) -> Option<String> {
        let place = self.history.peek(back, |p| self.can_show(p))?;
        if place.project.is_some() {
            return Some("Project".into());
        }
        if place.home {
            return None;
        }
        place.thread.and_then(|id| self.threads.get(&id).map(|t| t.place()))
    }

    /// What the card says, for both apps' dumps (`pageSwipe`): its lines,
    /// whether letting go goes, and how far along it is.
    #[cfg(feature = "automation")]
    pub(crate) fn page_swipe_dump(&self) -> serde_json::Value {
        let swipe = &self.swipe_nav.swipe;
        let Some(direction) = swipe.direction() else { return serde_json::Value::Null };
        let back = direction == Direction::Back;
        let place = self.destination(back);
        let empty = swipe.is_empty() || place.is_none();
        let mut lines = Vec::new();
        if !empty {
            lines.push(if back { "Back to".to_owned() } else { "Forward to".to_owned() });
        }
        lines.push(place.unwrap_or_else(|| if back { "Nothing to go back to".into() } else { "Nothing to go forward to".into() }));
        if let Some(detail) = (!empty).then(|| self.destination_detail(back)).flatten() {
            lines.push(detail);
        }
        serde_json::json!({ "lines": lines, "armed": swipe.armed(), "progress": swipe.progress() })
    }
}

/// The automation's `page-swipe`, as the Swift app's: two fingers across the
/// middle of the window (`{"dx": 150, "dy": 0}`, the content's way, right and
/// down positive; right is Back), as trackpad scrolls through the window's own
/// dispatch, a sixtieth of a second apart. `"lift": false` leaves the fingers
/// down, for a picture of the card; a later page-swipe carries on. Says how
/// far it got and where the window is after.
#[cfg(feature = "automation")]
pub(crate) async fn page_swipe_command(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
    let app = app.upgrade().ok_or("the app has gone")?;
    let (dx, dy) = (cmd["dx"].as_f64().unwrap_or(0.), cmd["dy"].as_f64().unwrap_or(0.));
    let steps = cmd["steps"].as_u64().unwrap_or(8).max(1);
    // Right of the middle, so over the thread rather than the sidebar.
    let at = cx
        .update_window(window, |_, window, cx| {
            let size = window.viewport_size();
            let at = point(size.width * 0.6, size.height * 0.45);
            crate::thread::scripted_input(|| window.pointer_move(at, None, cx));
            at
        })
        .map_err(|e| e.to_string())?;
    let send = |x: f64, y: f64, phase: TouchPhase, cx: &mut AsyncApp| {
        cx.update_window(window, |_, window, cx| {
            let event = ScrollEvent { position: at, delta: ScrollDelta::Pixels(point(gpui_kit::px(x as f32), gpui_kit::px(y as f32))), modifiers: Modifiers::default(), touch_phase: phase, source: ScrollSource::Pointer };
            crate::thread::scripted_input(|| {
                window.dispatch_event(event.to_platform_input(), cx);
                window.render_frame(cx);
            });
        })
        .map_err(|e| e.to_string())
    };
    // A new gesture unless one's under way (a page-swipe that didn't lift).
    let fresh = app.read_with(cx, |t, _| t.swipe_nav.swipe.direction().is_none());
    if fresh {
        send(0., 0., TouchPhase::Started, cx)?;
    }
    for _ in 0..steps {
        send(dx / steps as f64, dy / steps as f64, TouchPhase::Moved, cx)?;
        cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
    }
    let along = app
        .read_with(cx, |t, _| {
            let swipe = &t.swipe_nav.swipe;
            let way = match swipe.direction() {
                Some(Direction::Back) => "back",
                Some(Direction::Forward) => "forward",
                None => "no swipe",
            };
            format!("{way} progress {:.2} armed {}", swipe.progress(), swipe.armed())
        });
    if cmd["lift"].as_bool() == Some(false) {
        return Ok(serde_json::json!(format!("ok {along}")));
    }
    send(0., 0., TouchPhase::Ended, cx)?;
    cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
    let after = app.read_with(cx, |t, _| format!("{} back={} forward={}", t.selected.clone().unwrap_or_else(|| "new".into()), t.can_go_back(), t.can_go_forward()));
    Ok(serde_json::json!(format!("ok {along}, let go: {after}")))
}
