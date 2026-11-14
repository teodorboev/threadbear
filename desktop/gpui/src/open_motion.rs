// A thread coming in as it opens, the new-thread page as it comes up, the box
// dropping from that page to its place under the new thread, and a title
// fading over to its new name: the motion Mike picked from mockups on 13 Nov
// 2026 (~/dev/me/misc/threadbear-juice-mockups, gpui-switch.html,
// gpui-keys.html and gpui-new-thread.html; Threadbear review 737b22bb).
//
// All of it is short, as it happens many times a day: nothing longer than
// 0.22 s, and nothing waits on it. A thread is drawn in its place, scrolled
// to its end, from the first frame; it only starts a little faint and a few
// points off and eases the rest of the way, so a click never feels slower
// than it did before. The sidebar's highlight sliding to the row and the
// send lift are the bears-and-motion work's (row_motion.rs, composer_send.rs).
//
// Reduce Motion turns all of it off.
use crate::style::px;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How a thread's being opened, which says how it comes in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Opening {
    /// A click on its row, a link, a menu or a note: rises a few points as
    /// it fades up.
    Click,
    /// The arrow keys: comes in from the way the list was walked, 1 for
    /// down (from below) and -1 for up (from above).
    Step(i32),
    /// Sent from the new-thread page: the transcript is the message lifting
    /// off the box (the send lift), and the box drops from where it was on
    /// the page (its top, in the window) to its place under the transcript.
    FromNewPage { box_top: f32 },
    /// Something else moves it already: the archive flight fades the next
    /// thread in under the page, and a thread brought back from a panel of
    /// its own was on screen a moment ago.
    Still,
}

impl Opening {
    /// What the dump calls it, for the specs (features/opening.feature).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Opening::Click => "click",
            Opening::Step(dir) if dir < 0 => "up",
            Opening::Step(_) => "down",
            Opening::FromNewPage { .. } => "newPage",
            Opening::Still => "still",
        }
    }
}

/// The transcript coming in from a click (the mockup's 0.13 s).
const CLICK_IN: Duration = Duration::from_millis(130);
/// And from the arrow keys (0.14 s): a little further to come.
const STEP_IN: Duration = Duration::from_millis(140);
/// The new-thread page coming up (0.12 s).
const PAGE_IN: Duration = Duration::from_millis(120);
/// The box dropping from the new-thread page to its place (0.2 s).
const BOX_DROP: Duration = Duration::from_millis(200);
/// A title fading out, then its new name fading in.
const TITLE_OUT: Duration = Duration::from_millis(90);
const TITLE_IN: Duration = Duration::from_millis(130);

/// How faint a thread starts: it's never blank, so a click shows at once.
const START_OPACITY: f32 = 0.25;
/// How far below its place a clicked thread starts.
const CLICK_RISE: f32 = 6.;
/// How far off an arrowed-to thread starts, the way it was walked.
const STEP_SHIFT: f32 = 10.;
/// How far below its place the new-thread page starts.
const PAGE_RISE: f32 = 4.;

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Snappier, for the box's drop: most of the way there at once, settling.
fn ease_out_quint(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(5)
}

/// How far through `length` `elapsed` is, while it's still going.
fn progress(elapsed: Duration, length: Duration) -> Option<f32> {
    let t = elapsed.as_secs_f32() / length.as_secs_f32();
    (t < 1.).then_some(t.max(0.))
}

/// The transcript `elapsed` after it was first drawn: its opacity, and how
/// far from its place it's drawn (down is positive). None once it's there,
/// or for an opening that doesn't move it.
pub(crate) fn transcript_at(opening: Opening, elapsed: Duration) -> Option<(f32, f32)> {
    let (length, shift) = match opening {
        Opening::Click => (CLICK_IN, CLICK_RISE),
        Opening::Step(dir) => (STEP_IN, STEP_SHIFT * dir.signum() as f32),
        Opening::FromNewPage { .. } | Opening::Still => return None,
    };
    let e = ease_out_cubic(progress(elapsed, length)?);
    Some((START_OPACITY + (1. - START_OPACITY) * e, shift * (1. - e)))
}

/// The new-thread page `elapsed` after it came up: its opacity and how far
/// below its place it is.
pub(crate) fn page_at(elapsed: Duration) -> Option<(f32, f32)> {
    let e = ease_out_cubic(progress(elapsed, PAGE_IN)?);
    Some((e, PAGE_RISE * (1. - e)))
}

/// How far from its place the box is drawn, `elapsed` into its drop from
/// `delta` away.
pub(crate) fn box_drop_at(delta: f32, elapsed: Duration) -> Option<f32> {
    let e = ease_out_quint(progress(elapsed, BOX_DROP)?);
    Some(delta * (1. - e))
}

/// A title changing, `t` of the way through: the old one's opacity, then
/// the new one's. The old goes first so the two never sit on top of each
/// other at half strength, which reads as a smudge.
fn title_opacities(t: f32) -> (f32, f32) {
    let total = (TITLE_OUT + TITLE_IN).as_secs_f32();
    let at = t * total;
    let out = TITLE_OUT.as_secs_f32();
    if at < out {
        (1. - ease_out_cubic(at / out), 0.)
    } else {
        (0., ease_out_cubic((at - out) / TITLE_IN.as_secs_f32()))
    }
}

thread_local! {
    /// Until when something here is on the move, for the specs to wait out
    /// (`Threadbear::moving`). The window draws on one thread.
    static MOVING_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Titles changing, by thread and where they're shown (as
    /// row_motion.rs's counts).
    static TITLES: RefCell<Titles> = RefCell::new(Titles::default());
}

/// At most this many titles are remembered at once. The app lets go of a
/// thread's as the thread goes (`forget_titles`), so this is only a
/// backstop: another runner's rows come from a machine we trust less, and
/// one churning fresh thread ids through the sidebar grew the map without
/// end (Daybreak 117, 13 Nov 2026). Past it, a new title is drawn plain.
const TITLES_KEPT: usize = 4096;

/// One title's last two texts and when it changed.
struct Title {
    before: SharedString,
    now: SharedString,
    changed: Instant,
}

/// The titles drawn so far, by (thread id, where it's shown).
#[derive(Default)]
struct Titles {
    map: HashMap<(String, String), Title>,
}

impl Titles {
    /// Notes `text` as the title shown at (`thread`, `place`): the one it
    /// fades from and when it changed, or None to draw it plain (first
    /// seen, or no room left).
    fn note(&mut self, thread: &str, place: &str, text: &SharedString, now: Instant, total: Duration) -> Option<(SharedString, Instant)> {
        let key = (thread.to_owned(), place.to_owned());
        let Some(entry) = self.map.get_mut(&key) else {
            if self.map.len() < TITLES_KEPT {
                self.map.insert(key, Title { before: text.clone(), now: text.clone(), changed: now.checked_sub(total).unwrap_or(now) });
            }
            return None;
        };
        if entry.now != *text {
            entry.before = std::mem::replace(&mut entry.now, text.clone());
            entry.changed = now;
        } else if entry.before != entry.now && now.duration_since(entry.changed) >= total {
            // Faded over: the old text isn't needed any more.
            entry.before = entry.now.clone();
        }
        Some((entry.before.clone(), entry.changed))
    }

    fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.map.retain(|(thread, _), _| keep(thread));
    }
}

/// Lets go of the titles of threads that have gone (deleted, or another
/// runner's rows that left its index), so none outlives its thread.
pub(crate) fn forget_titles(keep: impl Fn(&str) -> bool) {
    TITLES.with(|titles| titles.borrow_mut().retain(keep));
}

/// Something's moving for a frame or so more.
fn note_moving() {
    let until = Instant::now() + Duration::from_millis(40);
    MOVING_UNTIL.with(|m| m.set(Some(m.get().map_or(until, |was| was.max(until)))));
}

/// Whether a thread, the page or a box is still on its way in.
pub(crate) fn moving() -> bool {
    MOVING_UNTIL.with(|m| m.get().is_some_and(|until| Instant::now() < until))
}

/// The app's side: how the next thread opens, and the new-thread page's.
#[derive(Default)]
pub(crate) struct Openings {
    /// How the next `open` comes in; a click unless whoever opens it says.
    pub(crate) next: Option<Opening>,
    /// The last thread opened and how, for the dump.
    pub(crate) last: Option<(String, Opening)>,
    /// When the new-thread page came up, while it's up.
    pub(crate) page_shown: Option<Instant>,
    /// Where the new-thread page's box is, in the window: the drop's start.
    pub(crate) box_top: Rc<Cell<Option<f32>>>,
}

impl Openings {
    /// How this open comes in, used up.
    pub(crate) fn take(&mut self) -> Opening {
        self.next.take().unwrap_or(Opening::Click)
    }

    /// The new-thread page, if it's still coming up: its opacity and rise,
    /// starting its clock the first time it's drawn.
    pub(crate) fn page(&mut self, reduce: bool, window: &mut Window) -> Option<(f32, f32)> {
        let shown = *self.page_shown.get_or_insert_with(Instant::now);
        if reduce {
            return None;
        }
        let at = page_at(shown.elapsed())?;
        window.request_animation_frame();
        note_moving();
        Some(at)
    }

    /// Notes where the page's box is as it's drawn, for the drop.
    pub(crate) fn box_marker(&self) -> impl IntoElement {
        let top = self.box_top.clone();
        canvas(move |bounds, _, _| top.set(Some(f32::from(bounds.origin.y))), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
    }
}

/// A thread pane's side: its transcript and box on their way in.
#[derive(Default)]
pub(crate) struct Entering {
    opening: Option<Opening>,
    /// When its transcript was first drawn with its rows.
    started: Option<Instant>,
    drop: Option<Rc<BoxDrop>>,
}

impl Entering {
    pub(crate) fn new(opening: Opening, reduce: bool) -> Self {
        if reduce {
            return Self::default();
        }
        let drop = match opening {
            Opening::FromNewPage { box_top } => Some(Rc::new(BoxDrop { from: box_top, delta: Cell::new(None), started: Cell::new(None) })),
            _ => None,
        };
        Entering { opening: Some(opening), started: None, drop }
    }

    /// The transcript's opacity and offset this frame, once it has rows to
    /// show (a thread that has to load first comes in when it's loaded).
    pub(crate) fn transcript(&mut self, loaded: bool, window: &mut Window) -> Option<(f32, f32)> {
        let opening = self.opening?;
        if !loaded {
            return None;
        }
        let started = *self.started.get_or_insert_with(Instant::now);
        match transcript_at(opening, started.elapsed()) {
            Some(at) => {
                window.request_animation_frame();
                note_moving();
                Some(at)
            }
            None => {
                self.opening = None;
                None
            }
        }
    }

    /// How far the box dropped from the new-thread page, once it's been
    /// laid out under the transcript (negative: it came down). For the dump.
    pub(crate) fn box_drop(&self) -> Option<f32> {
        self.drop.as_ref().and_then(|d| d.delta.get()).map(f32::round)
    }

    /// The box, painted where its drop has got to.
    pub(crate) fn wrap_box(&self, child: AnyElement) -> AnyElement {
        Shift { drop: self.drop.clone(), child }.into_any_element()
    }
}

/// The box's drop: where it started in the window, and once it's been laid
/// out in its new place, how far that is and when it set off.
pub(crate) struct BoxDrop {
    from: f32,
    delta: Cell<Option<f32>>,
    started: Cell<Option<Instant>>,
}

impl BoxDrop {
    fn offset(&self, top: f32) -> f32 {
        // Measured the first time it's laid out in its new place, and kept:
        // the drop runs once, from the page to there.
        let delta = self.delta.get().unwrap_or(self.from - top);
        self.delta.set(Some(delta));
        let started = self.started.get().unwrap_or_else(Instant::now);
        self.started.set(Some(started));
        box_drop_at(delta, started.elapsed()).unwrap_or(0.)
    }
}

/// An element laid out where it is and painted where its drop has got to,
/// as row_motion.rs's Glide: GPUI has no matched geometry.
struct Shift {
    drop: Option<Rc<BoxDrop>>,
    child: AnyElement,
}

impl IntoElement for Shift {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Shift {
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
        let offset = self.drop.as_ref().map_or(0., |d| d.offset(f32::from(bounds.origin.y)));
        if offset != 0. {
            window.request_animation_frame();
            note_moving();
        }
        let child = &mut self.child;
        window.with_element_offset(point(gpui_kit::px(0.), gpui_kit::px(offset)), |window| child.prepaint(window, cx));
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}

/// A title that fades over to its new text when it changes (the agent
/// naming a thread, a rename): `thread` is whose it is and `place` where
/// it's shown. Plain text the rest of the time, and always with Reduce
/// Motion, which remembers nothing.
pub(crate) fn title(thread: &str, place: &str, text: SharedString, reduce: bool) -> AnyElement {
    if reduce {
        return text.into_any_element();
    }
    let now = Instant::now();
    let total = TITLE_OUT + TITLE_IN;
    let Some((before, changed)) = TITLES.with(|titles| titles.borrow_mut().note(thread, place, &text, now, total)) else {
        return text.into_any_element();
    };
    if before == text || changed.elapsed() >= total {
        return text.into_any_element();
    }
    let key = format!("{thread}-{place}");
    note_moving();
    let id = format!("title-{key}-{changed:?}");
    div()
        .relative()
        .min_w_0()
        .truncate()
        .child(div().min_w_0().truncate().child(text).with_animation(SharedString::from(format!("{id}-in")), Animation::new(total), |el, t| el.opacity(title_opacities(t).1)))
        .child(div().absolute().top_0().left_0().right_0().truncate().child(before).with_animation(SharedString::from(format!("{id}-out")), Animation::new(total), |el, t| el.opacity(title_opacities(t).0)))
        .into_any_element()
}

/// The transcript's place: clips it while it's off by a few points, so it
/// never shows over the header or the box.
pub(crate) fn transcript_frame(inner: Div, at: Option<(f32, f32)>) -> Div {
    let inner = match at {
        Some((opacity, dy)) => inner.opacity(opacity).top(px(dy)),
        None => inner,
    };
    div().relative().flex_1().min_h_0().overflow_hidden().child(inner)
}

#[cfg(test)]
mod tests {
    // By name: a glob would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{box_drop_at, page_at, title_opacities, transcript_at, BoxDrop, Opening, Titles, BOX_DROP, CLICK_IN, CLICK_RISE, PAGE_IN, PAGE_RISE, START_OPACITY, STEP_IN, STEP_SHIFT, TITLES_KEPT, TITLE_IN, TITLE_OUT};
    use gpui_kit::SharedString;
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn a_clicked_thread_starts_faint_and_low_and_settles_in_place() {
        let (opacity, dy) = transcript_at(Opening::Click, MS(0)).unwrap();
        assert_eq!(opacity, START_OPACITY, "never blank, so the click shows at once");
        assert_eq!(dy, CLICK_RISE);
        let (mid_o, mid_dy) = transcript_at(Opening::Click, MS(65)).unwrap();
        assert!(mid_o > 0.8 && mid_dy < 1.5, "eased out: most of the way there by halfway ({mid_o}, {mid_dy})");
        assert_eq!(transcript_at(Opening::Click, CLICK_IN), None);
    }

    #[test]
    fn the_arrow_keys_bring_it_in_from_the_way_the_list_was_walked() {
        assert_eq!(transcript_at(Opening::Step(1), MS(0)).unwrap().1, STEP_SHIFT, "down: from below");
        assert_eq!(transcript_at(Opening::Step(-1), MS(0)).unwrap().1, -STEP_SHIFT, "up: from above");
        assert_eq!(transcript_at(Opening::Step(1), STEP_IN), None);
    }

    #[test]
    fn nothing_this_short_takes_longer_than_a_quarter_second() {
        for length in [CLICK_IN, STEP_IN, PAGE_IN, BOX_DROP, TITLE_OUT + TITLE_IN] {
            assert!(length <= MS(250), "{length:?}: Mike's limit for something done many times a day");
        }
    }

    #[test]
    fn the_archive_flight_and_the_new_page_leave_the_transcript_alone() {
        assert_eq!(transcript_at(Opening::Still, MS(0)), None);
        assert_eq!(transcript_at(Opening::FromNewPage { box_top: 300. }, MS(0)), None);
    }

    #[test]
    fn the_page_fades_up_from_a_little_below() {
        assert_eq!(page_at(MS(0)), Some((0., PAGE_RISE)));
        assert_eq!(page_at(PAGE_IN), None);
    }

    #[test]
    fn the_box_drops_the_whole_way_and_stops() {
        assert_eq!(box_drop_at(-240., MS(0)), Some(-240.));
        assert!(box_drop_at(-240., MS(100)).unwrap().abs() < 240. * 0.05, "quint: nearly there by halfway");
        assert_eq!(box_drop_at(-240., BOX_DROP), None);
    }

    #[test]
    fn the_box_drop_measures_from_its_new_place_once() {
        let drop = BoxDrop { from: 300., delta: Cell::new(None), started: Cell::new(None) };
        let first = drop.offset(620.);
        assert!((first - -320.).abs() < 1., "starts where it was on the page: {first}");
        // Laid out again in the same place: the same drop, not a new one.
        assert!(drop.offset(620.) <= 0.);
        assert_eq!(drop.delta.get(), Some(-320.));
    }

    #[test]
    fn a_title_fades_out_before_its_new_name_fades_in() {
        assert_eq!(title_opacities(0.), (1., 0.));
        let out_end = TITLE_OUT.as_secs_f32() / (TITLE_OUT + TITLE_IN).as_secs_f32();
        let (old, new) = title_opacities(out_end * 0.5);
        assert!(old > 0. && new == 0., "only the old one at first ({old}, {new})");
        let (old, new) = title_opacities(0.8);
        assert!(old == 0. && new > 0., "only the new one after ({old}, {new})");
        assert_eq!(title_opacities(1.), (0., 1.));
    }

    #[test]
    fn opening_names_for_the_dump() {
        assert_eq!(Opening::Click.name(), "click");
        assert_eq!(Opening::Step(1).name(), "down");
        assert_eq!(Opening::Step(-1).name(), "up");
        assert_eq!(Opening::FromNewPage { box_top: 0. }.name(), "newPage");
        assert_eq!(Opening::Still.name(), "still");
    }

    #[test]
    fn a_title_fades_from_its_old_name_then_lets_the_old_one_go() {
        let total = TITLE_OUT + TITLE_IN;
        let mut titles = Titles::default();
        let t0 = Instant::now();
        let (old, new): (SharedString, SharedString) = ("New thread".into(), "Fix the bug".into());
        assert!(titles.note("t", "header", &old, t0, total).is_none(), "first seen: drawn plain");
        assert_eq!(titles.note("t", "header", &old, t0, total).unwrap().0, old);
        let (before, changed) = titles.note("t", "header", &new, t0 + MS(5), total).unwrap();
        assert_eq!((before, changed), (old, t0 + MS(5)));
        let (before, _) = titles.note("t", "header", &new, t0 + MS(5) + total, total).unwrap();
        assert_eq!(before, new, "once it's faded over, only the new text is kept");
    }

    /// Daybreak 117: another runner churning fresh thread ids through the
    /// sidebar grew the map without end. Threads that went are let go, and
    /// the map never passes its cap whatever comes.
    #[test]
    fn titles_of_threads_that_went_are_let_go_and_the_map_stays_bounded() {
        let total = TITLE_OUT + TITLE_IN;
        let now = Instant::now();
        let mut titles = Titles::default();
        let text: SharedString = "x".repeat(300).into();
        for round in 0..20 {
            let ids: Vec<String> = (0..1000).map(|i| format!("{round}-{i}")).collect();
            for id in &ids {
                titles.note(id, "row-inbox", &text, now, total);
                titles.note(id, "header", &text, now, total);
            }
            // The rows leave the index: the app keeps only threads it still has.
            titles.retain(|_| false);
            assert!(titles.map.is_empty(), "round {round}: nothing outlives its thread");
        }
        // Never told they went: the cap still holds, and a stable key is one entry.
        for i in 0..(TITLES_KEPT * 3) {
            titles.note(&format!("fresh-{i}"), "row-inbox", &text, now, total);
        }
        assert_eq!(titles.map.len(), TITLES_KEPT);
        assert!(titles.note("past-the-cap", "row-inbox", &text, now, total).is_none(), "past the cap a title is just drawn plain");
        assert_eq!(titles.map.len(), TITLES_KEPT);
        titles.retain(|id| id == "fresh-1");
        for _ in 0..10_000 {
            titles.note("fresh-1", "row-inbox", &text, now, total);
        }
        assert_eq!(titles.map.len(), 1);
    }
}
