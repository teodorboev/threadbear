// A two-finger swipe across the window going back or forward, as in Safari, a
// port of ThreadbearCore's NavigationSwipe: content moved right goes back,
// left forward (with natural scrolling the fingers' way, without it the
// other way). It works like a sidebar row's swipe (row_swipe.rs): a card
// follows the fingers, and let go past the line (or flicked) it goes. Fed the
// trackpad's scroll events by desktop/gpui/src/swipe_nav.rs; this decides
// whether a gesture is a page swipe or a scroll and how far along it is, so
// it can be tested without a trackpad.
use crate::row_swipe;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// Fingers down, not yet clear which way.
    Deciding,
    /// Across, a way there's somewhere to go: the events are ours.
    Tracking(Direction),
    /// A scroll, or across with nowhere to go: left alone.
    Ignoring,
}

/// What an event means for whoever's feeding them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not clear yet.
    Wait,
    /// A scroll: it, and the rest of the gesture, aren't ours.
    Scroll,
    /// A page swipe, this event included.
    Swipe,
}

/// How far the fingers go before letting go goes there. 110 at first, then
/// 150; Mike found both too sensitive (8 and 9 Nov).
pub const THRESHOLD: f64 = 220.;
/// Moving this fast the swipe's way at the end is a flick: it goes even short
/// of the line, once it's `FLICK_TRAVEL` along. In points a second, as how far
/// one event moves depends on how often the trackpad sends them.
const FLICK_SPEED: f64 = 1200.;
const FLICK_TRAVEL: f64 = 100.;
/// How far across before a gesture that's being held is a swipe. Small, as
/// nothing happens yet but the card starting to show.
const ACROSS: f64 = 5.;
/// How far up or down before a sideways start is a scroll after all.
const UP_AND_DOWN: f64 = 4.;

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationSwipe {
    pub phase: Phase,
    /// How far the fingers have gone the swipe's way since they touched.
    pub travel: f64,
    /// Whether the events are kept from the view under the pointer while
    /// deciding. Not over a sidebar row, which needs the whole gesture.
    pub holds: bool,
    allowed: Vec<Direction>,
    /// Ways it can swipe with nowhere in the history to go: the card says so
    /// and springs back, rather than nothing showing at all, which Mike took
    /// for Forward not working (8 Nov).
    empty: Vec<Direction>,
    sum_x: f64,
    sum_y: f64,
    /// Points a second the swipe's way, at the last movement.
    last_speed: f64,
}

impl Default for NavigationSwipe {
    fn default() -> Self {
        Self { phase: Phase::Idle, travel: 0., holds: true, allowed: vec![], empty: vec![], sum_x: 0., sum_y: 0., last_speed: 0. }
    }
}

impl NavigationSwipe {
    pub fn direction(&self) -> Option<Direction> {
        match self.phase {
            Phase::Tracking(direction) => Some(direction),
            _ => None,
        }
    }

    /// 0 at the start, 1 at the line, for drawing the card.
    pub fn progress(&self) -> f64 {
        if self.direction().is_none() {
            return 0.;
        }
        let along = (self.travel / THRESHOLD).clamp(0., 1.);
        // With nowhere to go it only gives a little, as a row does.
        if self.is_empty() {
            along * 0.25
        } else {
            along
        }
    }

    /// Swiping a way with nowhere to go: it never goes.
    pub fn is_empty(&self) -> bool {
        self.direction().is_some_and(|d| self.empty.contains(&d))
    }

    /// Far enough along that letting go goes there.
    pub fn armed(&self) -> bool {
        self.direction().is_some() && !self.is_empty() && self.travel >= THRESHOLD && self.travel >= self.sum_y.abs()
    }

    /// Fingers touched the trackpad, with the ways there's somewhere to go.
    /// `empty`: more ways it can swipe, with nowhere to go.
    pub fn begin(&mut self, allowed: &[Direction], empty: &[Direction], holds: bool) {
        self.empty = empty.iter().copied().filter(|d| !allowed.contains(d)).collect();
        self.allowed = allowed.to_vec();
        self.phase = Phase::Deciding;
        self.travel = 0.;
        self.sum_x = 0.;
        self.sum_y = 0.;
        self.last_speed = 0.;
        self.holds = holds;
    }

    /// The fingers moved the content `dx` (right positive, so Back) and `dy`,
    /// as the system scrolls it, `elapsed` seconds after the last event.
    pub fn move_by(&mut self, dx: f64, dy: f64, elapsed: f64) -> Verdict {
        // Coalesced events can arrive all but together.
        let elapsed = elapsed.max(1. / 240.);
        match self.phase {
            Phase::Idle | Phase::Ignoring => Verdict::Scroll,
            Phase::Deciding => {
                self.sum_x += dx;
                self.sum_y += dy;
                let verdict = self.decide();
                if verdict == Verdict::Scroll {
                    self.phase = Phase::Ignoring;
                }
                if verdict == Verdict::Swipe {
                    self.last_speed = dx.abs() / elapsed;
                }
                verdict
            }
            Phase::Tracking(way) => {
                let along = if way == Direction::Back { dx } else { -dx };
                self.travel = (self.travel + along).max(0.);
                self.sum_y += dy;
                self.last_speed = along / elapsed;
                Verdict::Swipe
            }
        }
    }

    fn decide(&mut self) -> Verdict {
        if self.allowed.is_empty() && self.empty.is_empty() {
            return Verdict::Scroll;
        }
        let (x, y) = (self.sum_x.abs(), self.sum_y.abs());
        if self.holds {
            // Anything more across than up and down, as a real swipe drifts,
            // and a wobble of a point or two doesn't decide it.
            if y >= UP_AND_DOWN && y >= x {
                return Verdict::Scroll;
            }
            if x < ACROSS || x <= y {
                return Verdict::Wait;
            }
        } else {
            // Over a row: the row's own rule on the same events, so they
            // always agree whether it's across.
            if x.max(y) < row_swipe::DECIDE_AFTER {
                return Verdict::Wait;
            }
            if x < y * row_swipe::ACROSS {
                return Verdict::Scroll;
            }
        }
        let way = if self.sum_x > 0. { Direction::Back } else { Direction::Forward };
        if !self.allowed.contains(&way) && !self.empty.contains(&way) {
            return Verdict::Scroll;
        }
        self.phase = Phase::Tracking(way);
        self.travel = x;
        Verdict::Swipe
    }

    /// The fingers lifted: where to go, if anywhere. `still_moving`: they
    /// were moving right up to the lift; a quick move, a pause to look at the
    /// card and a lift isn't a flick.
    pub fn end(&mut self, still_moving: bool) -> Option<Direction> {
        let out = (|| {
            let direction = self.direction()?;
            if self.is_empty() {
                return None;
            }
            // A gesture that went on to be mostly up and down was a scroll
            // that started sideways, however far across it got.
            if self.travel < self.sum_y.abs() {
                return None;
            }
            if self.travel >= THRESHOLD {
                return Some(direction);
            }
            (still_moving && self.last_speed >= FLICK_SPEED && self.travel >= FLICK_TRAVEL).then_some(direction)
        })();
        self.reset();
        out
    }

    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.travel = 0.;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    const SIXTIETH: f64 = 1. / 60.;

    fn swiped(moves: &[(f64, f64)], allowed: &[Direction], holds: bool) -> NavigationSwipe {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(allowed, &[], holds);
        for &(dx, dy) in moves {
            swipe.move_by(dx, dy, SIXTIETH);
        }
        swipe
    }

    fn both(moves: &[(f64, f64)]) -> NavigationSwipe {
        swiped(moves, &[Back, Forward], true)
    }

    #[test]
    fn right_goes_back_and_left_forward() {
        let mut back = both(&[(8., 0.), (110., 0.), (110., 0.)]);
        assert_eq!(back.direction(), Some(Back));
        assert!(back.armed());
        assert_eq!(back.end(true), Some(Back));
        assert_eq!(back.phase, Phase::Idle);
        assert_eq!(both(&[(-8., 0.), (-230., 0.)]).end(true), Some(Forward));
    }

    #[test]
    fn across_is_a_swipe_within_a_few_points() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back], &[], true);
        assert_eq!(swipe.move_by(2., 0., SIXTIETH), Verdict::Wait);
        assert_eq!(swipe.move_by(4., 1., SIXTIETH), Verdict::Swipe);
        assert_eq!(swipe.move_by(20., 0., SIXTIETH), Verdict::Swipe);
        assert_eq!(swipe.travel, 26.);
    }

    #[test]
    fn a_swipe_that_drifts_is_still_a_swipe() {
        assert_eq!(both(&[(2., 1.), (2., 1.), (2., 1.)]).direction(), Some(Back));
    }

    #[test]
    fn up_and_down_scrolls() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back, Forward], &[], true);
        assert_eq!(swipe.move_by(1., -4., SIXTIETH), Verdict::Scroll);
        assert_eq!(swipe.move_by(200., 0., SIXTIETH), Verdict::Scroll);
        assert_eq!(swipe.end(true), None);
    }

    #[test]
    fn a_wobble_at_the_start_doesnt_decide_it() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back], &[], true);
        assert_eq!(swipe.move_by(0., 1., SIXTIETH), Verdict::Wait);
        assert_eq!(swipe.move_by(8., 0., SIXTIETH), Verdict::Swipe);
    }

    #[test]
    fn a_gesture_that_turns_mostly_vertical_goes_nowhere() {
        let mut swipe = both(&[(5., 4.), (20., 100.), (20., 100.)]);
        assert_eq!(swipe.direction(), Some(Back));
        assert_eq!(swipe.end(true), None);
        let mut far = both(&[(5., 4.), (120., 200.)]);
        assert!(!far.armed());
        assert_eq!(far.end(true), None);
    }

    #[test]
    fn a_sideways_start_that_turns_into_a_scroll_scrolls() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back], &[], true);
        assert_eq!(swipe.move_by(2., 0., SIXTIETH), Verdict::Wait);
        assert_eq!(swipe.move_by(1., 4., SIXTIETH), Verdict::Scroll);
    }

    #[test]
    fn short_or_brought_back_goes_nowhere() {
        let mut short = both(&[(8., 0.), (10., 0.), (10., 0.), (10., 0.), (10., 0.)]);
        assert!(!short.armed());
        assert_eq!(short.end(true), None);
        let mut back = both(&[(8., 0.), (150., 0.), (-100., 0.), (-3., 0.)]);
        assert!(!back.armed());
        assert_eq!(back.end(true), None);
    }

    #[test]
    fn a_flick_goes() {
        let mut swipe = both(&[(8., 0.), (60., 0.), (40., 0.)]);
        assert!(!swipe.armed());
        assert_eq!(swipe.end(true), Some(Back));
    }

    #[test]
    fn a_flick_doesnt_depend_on_how_events_are_batched() {
        let mut quick = NavigationSwipe::default();
        quick.begin(&[Back], &[], true);
        quick.move_by(8., 0., 1. / 120.);
        for _ in 0..10 {
            quick.move_by(10., 0., 1. / 120.);
        }
        assert_eq!(quick.end(true), Some(Back));
        let mut slow = NavigationSwipe::default();
        slow.begin(&[Back], &[], true);
        slow.move_by(8., 0., 0.05);
        for _ in 0..10 {
            slow.move_by(10., 0., 0.05);
        }
        assert_eq!(slow.end(true), None);
    }

    #[test]
    fn a_pause_before_lifting_isnt_a_flick() {
        assert_eq!(both(&[(8., 0.), (60., 0.), (40., 0.)]).end(false), None);
        assert_eq!(both(&[(8., 0.), (230., 0.)]).end(false), Some(Back), "past the line goes however slowly");
    }

    #[test]
    fn nowhere_in_the_history_tracks_but_never_goes() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back], &[Forward], true);
        assert_eq!(swipe.move_by(-8., 0., SIXTIETH), Verdict::Swipe);
        assert!(swipe.is_empty());
        swipe.move_by(-400., 0., SIXTIETH);
        assert!(!swipe.armed());
        assert!(swipe.progress() <= 0.25);
        assert_eq!(swipe.end(true), None);
    }

    #[test]
    fn nowhere_to_go_that_way_is_left_alone() {
        let mut nowhere = swiped(&[(-8., 0.), (-200., 0.)], &[Back], true);
        assert_eq!(nowhere.phase, Phase::Ignoring);
        assert_eq!(nowhere.end(true), None);
        assert_eq!(swiped(&[(8., 0.)], &[], true).phase, Phase::Ignoring);
    }

    #[test]
    fn over_a_row_it_decides_by_the_rows_rule() {
        let mut swipe = NavigationSwipe::default();
        swipe.begin(&[Back], &[], false);
        assert_eq!(swipe.move_by(5., 0., SIXTIETH), Verdict::Wait);
        assert_eq!(swipe.move_by(2., 0., SIXTIETH), Verdict::Swipe);
        assert_eq!(swiped(&[(5., 4.), (1., 1.)], &[Back], false).phase, Phase::Ignoring, "too diagonal for a row");
        let mut wobble = NavigationSwipe::default();
        wobble.begin(&[Back], &[], false);
        assert_eq!(wobble.move_by(0., 1., SIXTIETH), Verdict::Wait);
        assert_eq!(wobble.move_by(8., 0., SIXTIETH), Verdict::Swipe);
    }

    #[test]
    fn it_and_a_row_always_agree_whether_its_across() {
        for (dx, dy) in [(8., 0.), (7., 4.), (6., 4.), (9., 6.), (5., 5.), (0., 9.), (3., 8.), (6., 1.)] {
            let mut row = row_swipe::RowSwipe::default();
            row.begin(false);
            row.move_by(dx, dy);
            let mut page = NavigationSwipe::default();
            page.begin(&[Back], &[], false);
            page.move_by(dx, dy, SIXTIETH);
            assert_eq!(row.is_swiping(), page.direction().is_some(), "{dx}, {dy}");
        }
    }

    #[test]
    fn progress_runs_from_nothing_to_the_line() {
        assert!((both(&[(8., 0.)]).progress() - 8. / THRESHOLD).abs() < 0.001);
        assert_eq!(both(&[(8., 0.), (400., 0.)]).progress(), 1.);
        assert_eq!(both(&[(2., -8.)]).progress(), 0.);
    }
}
