// A two-finger swipe left on a sidebar row, as Mail does, a port of
// ThreadbearCore's RowSwipe: the row follows the fingers, and let go far
// enough along (or flicked) it archives. Fed the trackpad's scroll events by
// the row (desktop/gpui/src/row_swipe.rs), and a mouse dragging the row left
// too; this decides whether a gesture is a swipe at all and how far the row
// has gone, so it can be tested without a trackpad.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No gesture under way.
    Idle,
    /// Fingers down, not yet clear whether it's across or up and down.
    Deciding,
    /// Across: the row follows the fingers, and the events are the row's,
    /// not the list's.
    Tracking,
    /// Up and down: a scroll, left to the list until the fingers lift.
    Ignoring,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowSwipe {
    pub phase: Phase,
    /// How far the fingers have gone across since they touched, left
    /// negative, before any resistance.
    pub travel: f64,
    /// The row's width, which the distance to archive is a share of.
    pub width: f64,
    sum_x: f64,
    sum_y: f64,
    last_dx: f64,
    /// Points a second left at the last movement, when the events' times
    /// are known (`move_timed`).
    last_speed: Option<f64>,
    /// A swipe right is Back's or Forward's while there's somewhere to go,
    /// rather than the row giving a little and stopping.
    right_taken: bool,
}

/// Movement before a gesture is called across or up and down.
pub const DECIDE_AFTER: f64 = 6.;
/// Across has to be this much more than up and down to count, so a slightly
/// diagonal scroll still scrolls.
pub const ACROSS: f64 = 1.5;
/// One event moving this far left at the end is a flick: it archives even
/// short of the line, once the row's at least `FLICK_TRAVEL` along.
const FLICK_SPEED: f64 = 14.;
const FLICK_TRAVEL: f64 = 30.;
/// The same flick in points a second, for events whose times are known: 14
/// points in a sixtieth. How far one event moves depends on how often they
/// come, and an app that's busy for a moment is sent fewer, bigger ones,
/// which made a slow, short swipe a flick (seen with real trackpad events on
/// a loaded Tinker Desk, 12 Nov), as the page swipe found on 9 Nov.
const FLICK_POINTS_A_SECOND: f64 = FLICK_SPEED * 60.;

impl Default for RowSwipe {
    fn default() -> Self {
        Self::new(300.)
    }
}

impl RowSwipe {
    pub fn new(width: f64) -> Self {
        Self { phase: Phase::Idle, travel: 0., width, sum_x: 0., sum_y: 0., last_dx: 0., last_speed: None, right_taken: false }
    }

    /// How far along a let-go archives: a third of the row, and never less
    /// than 70 points, so a narrow sidebar doesn't archive on a nudge.
    pub fn threshold(&self) -> f64 {
        (self.width * 0.35).max(70.)
    }

    /// Where to draw the row: the travel to the left, up to the row's width;
    /// to the right only a little, with resistance, as there's nothing that way.
    pub fn offset(&self) -> f64 {
        if self.travel < 0. {
            self.travel.max(-self.width)
        } else {
            (self.travel * 0.2).min(24.)
        }
    }

    /// Far enough along that letting go archives.
    pub fn armed(&self) -> bool {
        self.phase == Phase::Tracking && self.travel <= -self.threshold()
    }

    pub fn is_swiping(&self) -> bool {
        self.phase == Phase::Tracking
    }

    /// A mouse dragging a row `dx`, `dy` from where it went down (left
    /// negative): true once that's the thread being dragged out, to drop into
    /// another thread's message, rather than this swipe. Any way but mostly
    /// left, by the same rule that decides a swipe, so the two never both start.
    pub fn drags_out(dx: f64, dy: f64) -> bool {
        if dx.abs().max(dy.abs()) < DECIDE_AFTER {
            return false;
        }
        !(dx < 0. && dx.abs() >= dy.abs() * ACROSS)
    }

    /// Fingers touched the trackpad. `right_taken`: a swipe right goes Back or
    /// Forward, so it isn't the row's.
    pub fn begin(&mut self, right_taken: bool) {
        self.phase = Phase::Deciding;
        self.right_taken = right_taken;
        self.travel = 0.;
        self.sum_x = 0.;
        self.sum_y = 0.;
        self.last_dx = 0.;
        self.last_speed = None;
    }

    /// The fingers moved (`dx` left negative, `dy` either way). Says whether
    /// the event is the row's: once it's tracking, so the list doesn't scroll
    /// while the row slides.
    pub fn move_by(&mut self, dx: f64, dy: f64) -> bool {
        match self.phase {
            Phase::Idle | Phase::Ignoring => false,
            Phase::Deciding => {
                self.sum_x += dx;
                self.sum_y += dy;
                if self.sum_x.abs().max(self.sum_y.abs()) < DECIDE_AFTER {
                    return false;
                }
                if self.sum_x.abs() >= self.sum_y.abs() * ACROSS && !(self.right_taken && self.sum_x > 0.) {
                    self.phase = Phase::Tracking;
                    self.travel = self.sum_x;
                    self.last_dx = dx;
                    return true;
                }
                self.phase = Phase::Ignoring;
                false
            }
            Phase::Tracking => {
                self.travel += dx;
                self.last_dx = dx;
                true
            }
        }
    }

    /// `move_by`, with how long it's been since the last movement, so a
    /// flick goes by how fast the fingers were going.
    pub fn move_timed(&mut self, dx: f64, dy: f64, elapsed: f64) -> bool {
        let mine = self.move_by(dx, dy);
        if self.phase == Phase::Tracking {
            // Never quicker than a frame apart: events an app was too busy to
            // take as they came arrive all but together, and aren't fast.
            self.last_speed = Some(-dx / elapsed.max(1. / 60.));
        }
        mine
    }

    /// `end`, knowing whether the fingers were still moving as they lifted:
    /// a quick move, a pause and a lift isn't a flick (`flickGap`, as the
    /// page swipe has it).
    pub fn end_moving(&mut self, still_moving: bool) -> bool {
        if !still_moving {
            self.last_dx = 0.;
            self.last_speed = Some(0.);
        }
        self.end()
    }

    /// The fingers lifted. True when the row should archive.
    pub fn end(&mut self) -> bool {
        let tracking = self.phase == Phase::Tracking;
        self.phase = Phase::Idle;
        if !tracking {
            return false;
        }
        if self.travel <= -self.threshold() {
            return true;
        }
        let flicked = match self.last_speed {
            Some(speed) => speed >= FLICK_POINTS_A_SECOND,
            None => self.last_dx <= -FLICK_SPEED,
        };
        flicked && self.travel <= -FLICK_TRAVEL
    }

    /// The gesture was cancelled (the system took it, say): never archives.
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.travel = 0.;
    }

    /// Back to rest after the row has sprung back or gone.
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.travel = 0.;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swiped(moves: &[(f64, f64)], width: f64) -> RowSwipe {
        let mut swipe = RowSwipe::new(width);
        swipe.begin(false);
        for &(dx, dy) in moves {
            swipe.move_by(dx, dy);
        }
        swipe
    }

    #[test]
    fn across_tracks_and_takes_the_events() {
        let mut swipe = RowSwipe::new(300.);
        swipe.begin(false);
        assert!(!swipe.move_by(-3., 0.), "too small to call yet");
        assert!(swipe.move_by(-4., 1.));
        assert!(swipe.is_swiping());
        assert_eq!(swipe.offset(), -7.);
        assert!(swipe.move_by(-20., 5.));
        assert_eq!(swipe.offset(), -27.);
    }

    #[test]
    fn up_and_down_is_left_to_the_list() {
        let mut swipe = RowSwipe::default();
        swipe.begin(false);
        assert!(!swipe.move_by(2., -8.));
        assert_eq!(swipe.phase, Phase::Ignoring);
        assert!(!swipe.move_by(-200., 0.));
        assert_eq!(swipe.offset(), 0.);
        assert!(!swipe.end());
    }

    #[test]
    fn a_mouse_drag_left_swipes_and_any_other_way_drags_the_thread_out() {
        assert!(!RowSwipe::drags_out(3., 2.), "too small to call yet");
        assert!(!RowSwipe::drags_out(-8., 1.), "left is the swipe");
        assert!(RowSwipe::drags_out(8., 1.), "right, towards the thread");
        assert!(RowSwipe::drags_out(0., -8.));
        assert!(RowSwipe::drags_out(1., 8.));
        assert!(RowSwipe::drags_out(-6., -5.), "a diagonal isn't a swipe either");
        // Whichever the swipe would have tracked, it isn't a drag out.
        for (dx, dy) in [(-6., 0.), (-7., 4.), (-30., -12.)] {
            assert_eq!(RowSwipe::drags_out(dx, dy), !swiped(&[(dx, dy)], 300.).is_swiping(), "{dx}, {dy}");
        }
    }

    #[test]
    fn a_diagonal_scrolls() {
        assert_eq!(swiped(&[(-6., -5.)], 300.).phase, Phase::Ignoring);
    }

    #[test]
    fn let_go_past_the_line_archives() {
        let mut short = swiped(&[(-10., 0.); 6], 300.);
        assert!(!short.armed());
        assert!(!short.end());
        let mut far = swiped(&[(-10., 0.), (-50., 0.), (-50., 0.)], 300.);
        assert!(far.armed());
        assert!(far.end());
        assert_eq!(far.phase, Phase::Idle);
    }

    #[test]
    fn a_timed_flick_goes_by_speed_however_the_events_come() {
        let timed = |moves: &[(f64, f64)]| {
            let mut swipe = RowSwipe::new(300.);
            swipe.begin(false);
            for &(dx, elapsed) in moves {
                swipe.move_timed(dx, 0., elapsed);
            }
            swipe.end()
        };
        // Short and slow, sent as a busy app gets it: two sixtieths at once,
        // or a few let through together.
        assert!(!timed(&[(-10., 1. / 30.), (-10., 1. / 30.), (-15., 1. / 30.), (-5., 1. / 30.)]));
        assert!(!timed(&[(-5., 1. / 60.), (-5., 0.001), (-5., 0.001), (-10., 0.001), (-10., 0.001), (-5., 0.001)]));
        // The same short way, fast at the end.
        assert!(timed(&[(-10., 1. / 60.), (-10., 1. / 60.), (-20., 1. / 60.)]));
        // Fast, but not far enough along yet.
        assert!(!timed(&[(-8., 1. / 60.), (-20., 1. / 60.)]));
    }

    #[test]
    fn a_pause_before_lifting_isnt_a_flick() {
        let mut paused = swiped(&[(-10., 0.), (-10., 0.), (-20., 0.)], 300.);
        assert!(!paused.end_moving(false));
        let mut moving = swiped(&[(-10., 0.), (-10., 0.), (-20., 0.)], 300.);
        assert!(moving.end_moving(true));
        let mut far = swiped(&[(-10., 0.), (-200., 0.)], 300.);
        assert!(far.end_moving(false), "past the line goes however slowly");
    }

    #[test]
    fn the_threshold_has_a_floor() {
        assert_eq!(RowSwipe::new(120.).threshold(), 70.);
        assert_eq!(RowSwipe::new(400.).threshold(), 140.);
    }

    #[test]
    fn coming_back_disarms() {
        let mut swipe = swiped(&[(-10., 0.), (-100., 0.), (60., 0.)], 300.);
        assert!(!swipe.armed());
        assert!(!swipe.end());
    }

    #[test]
    fn a_flick_archives_and_a_slow_drag_doesnt() {
        assert!(swiped(&[(-10., 0.), (-10., 0.), (-20., 0.)], 300.).end());
        assert!(!swiped(&[(-10., 0.), (-10., 0.), (-5., 0.), (-5., 0.), (-5., 0.)], 300.).end());
    }

    #[test]
    fn the_offset_has_its_limits() {
        assert_eq!(swiped(&[(10., 0.), (40., 0.)], 300.).offset(), 10.);
        assert_eq!(swiped(&[(10., 0.), (400., 0.)], 300.).offset(), 24.);
        assert_eq!(swiped(&[(-10., 0.), (-900., 0.)], 300.).offset(), -300.);
    }

    #[test]
    fn a_cancel_never_archives() {
        let mut swipe = swiped(&[(-10., 0.), (-200., 0.)], 300.);
        swipe.cancel();
        assert!(!swipe.end());
        assert_eq!(swipe.offset(), 0.);
    }

    #[test]
    fn a_swipe_right_is_left_to_back_when_its_taken() {
        let mut right = RowSwipe::default();
        right.begin(true);
        assert!(!right.move_by(8., 0.));
        assert_eq!(right.phase, Phase::Ignoring);
        let mut left = RowSwipe::default();
        left.begin(true);
        assert!(left.move_by(-8., 0.));
        assert!(left.is_swiping());
    }
}
