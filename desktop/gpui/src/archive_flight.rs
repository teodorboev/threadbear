// The open thread's page dropping into the Archived heading when it's
// archived, and sinking into Hibernating's moon when it's put to sleep, while
// the next thread fades in under it: the Swift app's `ArchiveFlight`
// (Views/ArchiveFlight.swift), whose path and timings are in
// threadbear_core::archive_flight. The heading's box bounces as it lands, and
// the thread's row and the heading's count wait for it there.
//
// The Swift app flies a picture of the transcript in Core Animation, off the
// main thread. GPUI can't take a picture of what it drew, nor scale or turn a
// view, so the page here is drawn: a card the page's colour with the shape of
// a transcript on it (a message's bubble, lines of a reply), scaled as the
// path says. It's drawn by a view of its own over the whole window
// (`FlightView`), so its frames redraw only that view and the window's root,
// never the transcript or the sidebar, whose views are cached: the same click
// opens the next thread, and that has the main thread's time. Where it's got to
// comes from the clock, from its first frame, not from counting frames: a
// frame the main thread misses mid-flight is skipped rather than slowing the
// flight down, and a click that holds the main thread before the first one
// (opening a heavy next thread) delays the flight rather than eating it.
use crate::app::Threadbear;
use crate::sidebar::{LEADING, OUTER_INSET};
use crate::style;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
pub(crate) use threadbear_core::archive_flight::Destination;
use threadbear_core::archive_flight::{self as flight, Path};

/// Where a flight can start or land, noted as each is painted (the Swift
/// app's `FlightMarker`s).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Mark {
    /// The Archived heading's box.
    ArchiveBox,
    /// The Hibernating heading's moon.
    HibernateBox,
    /// The open thread's row.
    OpenRow,
    /// The part of the sidebar's list in sight.
    List,
}

#[derive(Default)]
struct Marks {
    sidebar: HashMap<Mark, Bounds<Pixels>>,
    /// Each thread pane's transcript, by the pane's entity.
    transcripts: HashMap<EntityId, Bounds<Pixels>>,
}

thread_local! {
    // The window draws on one thread, and these are read on it too.
    static MARKS: RefCell<Marks> = RefCell::default();
}

/// The sidebar's list is being drawn again: what it marked last time may
/// not be there now (a folded section, a heading scrolled out of the list).
pub(crate) fn sidebar_drawing() {
    MARKS.with(|m| m.borrow_mut().sidebar.clear());
}

/// Notes where its parent is painted, if any of it is in sight (a heading
/// scrolled out of the list doesn't count, as `isInSight` in the Swift app).
pub(crate) fn mark(kind: Mark) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let visible = window.content_mask().bounds;
            if bounds.intersects(&visible) {
                let shown = if kind == Mark::List { bounds.intersect(&visible) } else { bounds };
                MARKS.with(|m| m.borrow_mut().sidebar.insert(kind, shown));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Notes where a pane's transcript is, for a flight to start from.
pub(crate) fn transcript_mark(pane: EntityId) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            MARKS.with(|m| {
                let mut m = m.borrow_mut();
                // Panes come and go with the threads opened; a few dozen
                // rectangles is nothing, but there's no need to keep them all.
                if m.transcripts.len() > 32 {
                    m.transcripts.clear();
                }
                m.transcripts.insert(pane, bounds);
            })
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn marked(kind: Mark) -> Option<Bounds<Pixels>> {
    MARKS.with(|m| m.borrow().sidebar.get(&kind).copied())
}

fn centre(b: Bounds<Pixels>) -> (f32, f32) {
    (f32::from(b.origin.x) + f32::from(b.size.width) / 2., f32::from(b.origin.y) + f32::from(b.size.height) / 2.)
}

fn rect_json(b: Bounds<Pixels>) -> Value {
    json!({ "x": f32::from(b.origin.x).round(), "y": f32::from(b.origin.y).round(), "width": f32::from(b.size.width).round(), "height": f32::from(b.size.height).round() })
}

/// The app's side of flights: which threads are on their way, and the
/// headings' landings, for their box to bounce.
#[derive(Default)]
pub(crate) struct Flights {
    view: Option<Entity<FlightView>>,
    /// Threads whose page is on its way to a heading: their row and the
    /// heading's count wait for it (`flyingToArchive`, `flyingToHibernate`).
    pub(crate) flying: HashMap<String, Destination>,
    /// Each heading's landings and when the last was, by section key.
    landings: HashMap<&'static str, (u32, Instant)>,
    /// The last flight, for scripts (`dump`'s `flight`).
    last: Option<Value>,
    serial: u64,
}

impl Flights {
    /// Whether a section's rows should leave this thread out for now.
    pub(crate) fn holds(&self, section: &str, id: &str) -> bool {
        match self.flying.get(id) {
            Some(Destination::Archive) => section == "archived",
            Some(Destination::Hibernate) => section == "hibernating",
            None => false,
        }
    }

    /// A page's on its way to its heading (the last flight hasn't landed).
    #[cfg(feature = "automation")]
    pub(crate) fn in_flight(&self) -> bool {
        self.last.as_ref().is_some_and(|l| l["landed"] == json!(false))
    }

    pub(crate) fn dump(&self) -> Value {
        let mut last = self.last.clone().unwrap_or(Value::Null);
        // A heading that only turns up as the page lands (the first thread
        // to hibernate) is measured the first time it's drawn after.
        if last["landed"] == json!(true) && last["landedBox"].is_null() {
            let kind = if last["to"] == json!("archive") { Mark::ArchiveBox } else { Mark::HibernateBox };
            last["landedBox"] = marked(kind).map(rect_json).unwrap_or(Value::Null);
        }
        last
    }

    /// The flights, over the whole window. Drawn by the shell.
    pub(crate) fn overlay(&mut self, cx: &mut App) -> AnyElement {
        let view = self.view.get_or_insert_with(|| cx.new(|_| FlightView::default())).clone();
        deferred(div().absolute().top_0().left_0().size_full().child(view)).with_priority(1).into_any_element()
    }

    /// A heading's box or moon, bouncing for a moment when a page lands in
    /// it: the box hops (`.symbolEffect(.bounce)`), the moon dips and lifts
    /// a shade (`.bounce.down` with a brightness phase).
    pub(crate) fn landing_icon(&self, key: &str, icon: gpui_kit::component::Icon, base: Hsla, lit: Hsla, reduce: bool) -> AnyElement {
        const BOUNCE: Duration = Duration::from_millis(500);
        let Some((count, at)) = self.landings.get(key).copied().filter(|(_, at)| at.elapsed() < BOUNCE * 2) else {
            return icon.into_any_element();
        };
        if reduce {
            return icon.into_any_element();
        }
        let moon = key == "hibernating";
        // Keyed by the landing, so each one plays it once; and only soon
        // after it, so a heading scrolled away and back doesn't play it again.
        icon.with_animation(SharedString::from(format!("landing-{key}-{count}-{at:?}")), Animation::new(BOUNCE), move |icon, t| {
            // Up and back with a little settle, as the symbol's bounce does.
            let lift = (std::f32::consts::PI * t).sin() * (1. - 0.5 * t);
            if moon {
                icon.transform(Transformation::translate(point(gpui_kit::px(0.), gpui_kit::px(style::zoomed(2.5) * lift)))).text_color(base.blend(lit.opacity(0.7 * lift)))
            } else {
                icon.transform(Transformation::scale(size(1. + 0.3 * lift, 1. + 0.3 * lift)))
            }
        })
        .into_any_element()
    }
}

impl Threadbear {
    /// Starts the open thread's page on its way to `to`'s heading, if it's the
    /// transcript showing in the main panel (not a sub-agent's, a project's
    /// page or a thread in a panel of its own). Call it before the thread
    /// changes and the next one opens, as it reads where things are drawn
    /// now. False when it didn't start.
    pub(crate) fn launch_flight(&mut self, id: &str, to: Destination, cx: &mut Context<Self>) -> bool {
        if self.selected.as_deref() != Some(id) || self.subagent_pane.is_some() || self.open_project().is_some() || self.home_page().is_some() {
            return false;
        }
        // The pane's entity, not the pane: an Archive on the done card is a
        // click in the pane, which is being updated while this runs.
        let Some(pane) = self.pane.as_ref().map(|p| p.entity_id()) else { return false };
        let Some(rect) = MARKS.with(|m| m.borrow().transcripts.get(&pane).copied()) else { return false };
        if f32::from(rect.size.width) <= 1. || f32::from(rect.size.height) <= 1. {
            return false;
        }
        let Some(thread) = self.threads.get(id) else { return false };
        // A starred thread stays in Starred however long it sleeps: nowhere to fly to.
        if to == Destination::Hibernate && (thread.is_starred() || thread.is_archived()) {
            return false;
        }
        let reduced = cx.reduce_motion();
        let box_mark = match to {
            Destination::Archive => marked(Mark::ArchiveBox),
            // With no Hibernating heading yet, its moon turns up where
            // Archived's box is now, pushing that down, so it aims there. One
            // that's there but out of sight isn't Archived's, so then it falls
            // off the list. Whether it's there comes from the threads: a
            // heading scrolled away isn't marked at all.
            Destination::Hibernate => {
                let now = crate::now_ms();
                let there = self.flight.flying.values().any(|d| *d == Destination::Hibernate) || self.shown_sections(cx, now).iter().any(|s| s.key == "hibernating" && s.rows.iter().any(|t| t.id != id));
                if there {
                    marked(Mark::HibernateBox)
                } else {
                    marked(Mark::ArchiveBox)
                }
            }
        };
        let open_row = marked(Mark::OpenRow).map(|b| (f32::from(b.origin.y), f32::from(b.size.height)));
        let list = marked(Mark::List).map(|b| (f32::from(b.origin.x), f32::from(b.origin.y) + f32::from(b.size.height)));
        let target = if reduced { None } else { flight::target(box_mark.map(centre), open_row, list, style::zoomed(OUTER_INSET + LEADING + 7.)) };
        let path = target.map(|(end, on_heading)| Path { start: centre(rect), end, swing: style::zoomed(to.swing()), on_heading, to });
        self.flight.serial += 1;
        let serial = self.flight.serial;
        let started = Rc::new(Cell::new(None));
        let card = Card { to, rect, path, started: started.clone(), seed: id.bytes().fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) };
        let lands_in = card.flight_time();
        self.flight.last = Some(json!({
            "to": to.name(),
            "thread": id,
            "start": rect_json(rect),
            "end": path.map(|p| json!({ "x": p.end.0.round(), "y": p.end.1.round() })),
            "onHeading": path.is_some_and(|p| p.on_heading),
            "landed": false,
            "landedBox": Value::Null,
        }));
        let view = self.flight.view.get_or_insert_with(|| cx.new(|_| FlightView::default())).clone();
        view.update(cx, |v, cx| {
            v.cards.push(card);
            cx.notify();
        });
        self.flight.flying.insert(id.to_owned(), to);
        let id = id.to_owned();
        let launched = Instant::now();
        cx.spawn(async move |this, cx| {
            // Lands when the page gets there, timed from its first frame:
            // the click can hold the main thread past when it was asked for.
            // One never drawn (a hidden window) lands a moment after.
            loop {
                let from = started.get().unwrap_or(launched);
                let left = lands_in.saturating_sub(from.elapsed());
                if left.is_zero() && (started.get().is_some() || launched.elapsed() > Duration::from_secs(1)) {
                    break;
                }
                cx.background_executor().timer(left.max(Duration::from_millis(16))).await;
            }
            this.update(cx, |this, cx| this.flight_landed(&id, to, serial, cx)).ok();
        })
        .detach();
        true
    }

    /// The page is there: the heading's box bounces, and the row and the
    /// count arrive.
    fn flight_landed(&mut self, id: &str, to: Destination, serial: u64, cx: &mut Context<Self>) {
        if self.flight.flying.get(id) == Some(&to) {
            self.flight.flying.remove(id);
        }
        let key = if to == Destination::Archive { "archived" } else { "hibernating" };
        let entry = self.flight.landings.entry(key).or_insert((0, Instant::now()));
        *entry = (entry.0 + 1, Instant::now());
        if serial == self.flight.serial {
            let landed_box = marked(if to == Destination::Archive { Mark::ArchiveBox } else { Mark::HibernateBox });
            if let Some(last) = self.flight.last.as_mut() {
                last["landed"] = json!(true);
                last["landedBox"] = landed_box.map(rect_json).unwrap_or(Value::Null);
            }
        }
        cx.notify();
    }
}

/// One page on its way.
#[derive(Clone)]
struct Card {
    to: Destination,
    /// The transcript it was, in the window.
    rect: Bounds<Pixels>,
    /// None: nowhere to go (no list in sight, or Reduce Motion), so it fades where it is.
    path: Option<Path>,
    /// Its first frame. Timed from there, not the click: the click goes on to
    /// open the next thread, and a long open used to eat the whole flight.
    started: Rc<Cell<Option<Instant>>>,
    /// Varies the transcript's shape a little from thread to thread.
    seed: u64,
}

impl Card {
    fn flight_time(&self) -> Duration {
        if self.path.is_some() {
            self.to.duration()
        } else {
            flight::FADE
        }
    }

    /// Until the page has gone and the next thread is all there.
    fn done(&self, elapsed: Duration) -> bool {
        elapsed >= self.flight_time() && flight::next_thread_shown(elapsed, self.to, self.path.is_none()) >= 1.
    }
}

#[derive(Default)]
pub(crate) struct FlightView {
    cards: Vec<Card>,
}

impl Render for FlightView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        for c in &self.cards {
            if c.started.get().is_none() {
                c.started.set(Some(now));
            }
        }
        self.cards.retain(|c| !c.done(now.duration_since(c.started.get().unwrap_or(now))));
        let theme = cx.theme();
        let (page, line, bubble, border) = (theme.background, theme.muted_foreground.opacity(0.22), theme.muted_foreground.opacity(0.12), theme.border.opacity(0.6));
        let mut layer = div().absolute().top_0().left_0().size_full();
        if !self.cards.is_empty() {
            window.request_animation_frame();
        }
        for card in &self.cards {
            let elapsed = now.duration_since(card.started.get().unwrap_or(now));
            // The next thread is hidden at first and fades in under the page
            // once it's on its way: a cover the page's colour over where the
            // transcript is, lifting.
            let shown = flight::next_thread_shown(elapsed, card.to, card.path.is_none());
            if shown < 1. {
                layer = layer.child(placed(card.rect, 0.).bg(page).opacity(1. - shown));
            }
            let t = (elapsed.as_secs_f32() / card.flight_time().as_secs_f32()).min(1.);
            if t >= 1. {
                continue;
            }
            let (frame, dimming) = match card.path {
                Some(path) => {
                    let f = path.at_time(t);
                    (f, f.dimming)
                }
                // A quick ease-in fade where it is.
                None => {
                    let (x, y) = centre(card.rect);
                    (flight::Frame { x, y, scale: 1., tilt: 0., corner_radius: 6., opacity: 1. - t * t, dimming: 0. }, 0.)
                }
            };
            let (w, h) = (f32::from(card.rect.size.width) * frame.scale, f32::from(card.rect.size.height) * frame.scale);
            let bounds = Bounds { origin: point(gpui_kit::px(frame.x - w / 2.), gpui_kit::px(frame.y - h / 2.)), size: size(gpui_kit::px(w), gpui_kit::px(h)) };
            let radius = frame.corner_radius * frame.scale;
            let mut el = placed(bounds, radius).bg(page).border_1().border_color(border).overflow_hidden().opacity(frame.opacity);
            el = el.child(transcript_shape(card, frame.scale, line, bubble));
            if dimming > 0. {
                // Dark rather than see-through: only black reads as the lights going down.
                el = el.child(div().absolute().top_0().left_0().size_full().bg(gpui_kit::black().opacity(dimming)));
            }
            layer = layer.child(el);
        }
        layer
    }
}

/// A box at `bounds` in the window.
fn placed(bounds: Bounds<Pixels>, radius: f32) -> Div {
    div().absolute().left(bounds.origin.x).top(bounds.origin.y).w(bounds.size.width).h(bounds.size.height).rounded(gpui_kit::px(radius))
}

/// What the page has on it, at its scale: the shape of a transcript (his
/// message in a bubble on the right, a few lines of reply under it), as the
/// Swift card's picture of the real one shrinks to.
fn transcript_shape(card: &Card, scale: f32, line: Hsla, bubble: Hsla) -> impl IntoElement {
    let z = |v: f32| gpui_kit::px(style::zoomed(v) * scale);
    let height = f32::from(card.rect.size.height) / style::zoomed(1.).max(0.01);
    let mut col = div().absolute().top_0().left_0().size_full().flex().flex_col().px(z(36.)).pt(z(24.));
    let mut seed = card.seed;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % 1000) as f32 / 1000.
    };
    let mut used = 24.;
    while used < height {
        let width = 0.35 + 0.25 * next();
        col = col.child(div().w_full().flex().flex_row().justify_end().child(div().w(relative(width)).h(z(30.)).rounded(z(12.)).bg(bubble)));
        used += 30. + 20.;
        let lines = 2 + (next() * 4.) as usize;
        for i in 0..lines {
            let width = if i + 1 == lines { 0.3 + 0.4 * next() } else { 0.85 + 0.15 * next() };
            col = col.child(div().mt(z(if i == 0 { 20. } else { 9. })).w(relative(width)).h(z(7.)).rounded(z(3.5)).bg(line));
            used += 16.;
        }
        col = col.child(div().h(z(26.)));
        used += 26.;
    }
    col
}
