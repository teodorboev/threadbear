// The open thread's page dropping into the Archived heading when it's
// archived, or sinking into Hibernating's moon when it's put to sleep: the
// Swift app's `ArchiveFlightPath` (ThreadbearCore) and the timings in its
// `ArchiveFlight.Destination` (Views/ArchiveFlight.swift), which Mike picked
// from mockups (30 Oct, "D2 · Drop"; 7 Nov for the hibernate). The drawing is
// desktop/gpui/src/archive_flight.rs; this is the path, kept apart so it can
// be tested without drawing anything.
//
// Coordinates here run downwards, as GPUI's do. The Swift path's run upwards
// (AppKit's window coordinates), so its control point's "short of the end's
// height" (end.y - swing there) is end.y + swing here: below the heading
// either way, so the page swings out and comes up into it.
use std::time::Duration;

/// Where a flight is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Falls, shrinks and swings across into the Archived heading's box.
    Archive,
    /// Sinks, level and dimming, into the Hibernating heading's moon.
    Hibernate,
}

impl Destination {
    /// How long the page takes to get there (`Destination.duration`).
    pub fn duration(self) -> Duration {
        Duration::from_millis(if self == Self::Archive { 440 } else { 520 })
    }
    /// How far the path's control point sits left of the start and below the
    /// end, in points before zoom.
    pub fn swing(self) -> f32 {
        if self == Self::Archive {
            40.
        } else {
            16.
        }
    }
    /// The next thread stays hidden this long, then fades in under the page.
    pub fn next_thread_delay(self) -> Duration {
        Duration::from_millis(if self == Self::Archive { 200 } else { 260 })
    }
    pub fn next_thread_fade(self) -> Duration {
        Duration::from_millis(if self == Self::Archive { 280 } else { 360 })
    }
    /// How far the next thread rises as it fades in, in points before zoom.
    /// The drop is quick enough to carry the change alone.
    pub fn next_thread_rise(self) -> f32 {
        if self == Self::Archive {
            0.
        } else {
            8.
        }
    }
    pub fn name(self) -> &'static str {
        if self == Self::Archive {
            "archive"
        } else {
            "hibernate"
        }
    }
}

/// With nowhere to fly to (no list in sight) or under Reduce Motion, the page
/// just fades where it is, this quickly, and the next thread comes in sooner.
pub const FADE: Duration = Duration::from_millis(180);
pub const REDUCED_NEXT_DELAY: Duration = Duration::from_millis(50);
pub const REDUCED_NEXT_FADE: Duration = Duration::from_millis(150);

/// The smallest the page gets, at the heading.
pub const SMALLEST: f32 = 0.02;

/// The page part way along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Its centre.
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    /// Radians, anticlockwise. GPUI can't turn a page, so the app leaves it
    /// level; kept so the path stays the Swift one.
    pub tilt: f32,
    /// In the page's own (unscaled) points.
    pub corner_radius: f32,
    pub opacity: f32,
    /// How far it's gone dark, 0 to 1 (hibernate only).
    pub dimming: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Path {
    pub start: (f32, f32),
    pub end: (f32, f32),
    pub swing: f32,
    /// True when `end` is the heading itself; false when it's only the bottom
    /// of the list, with the heading out of sight, where the page fades out
    /// sooner and all the way.
    pub on_heading: bool,
    pub to: Destination,
}

/// Eased time to progress: slow off the mark, quick through the middle,
/// settling into the heading. The hibernate's sine is softer than the
/// drop's cubic, with no snap through the middle.
pub fn progress(t: f32, to: Destination) -> f32 {
    let t = t.clamp(0., 1.);
    match to {
        Destination::Archive => {
            if t < 0.5 {
                4. * t * t * t
            } else {
                1. - (-2. * t + 2.).powi(3) / 2.
            }
        }
        Destination::Hibernate => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
    }
}

impl Path {
    /// The page at time `t` of its flight (0 to 1), eased.
    pub fn at_time(&self, t: f32) -> Frame {
        self.frame(progress(t, self.to))
    }

    /// The page `p` of the way along (0 at the start, 1 at the heading).
    pub fn frame(&self, p: f32) -> Frame {
        let p = p.clamp(0., 1.);
        let control = (self.start.0 - self.swing, self.end.1 + self.swing);
        let q = 1. - p;
        let x = q * q * self.start.0 + 2. * q * p * control.0 + p * p * self.end.0;
        let y = q * q * self.start.1 + 2. * q * p * control.1 + p * p * self.end.1;
        let corner_radius = (6. + 60. * p).min(40.);
        if self.to == Destination::Hibernate {
            // Shrinks a touch later than the drop: a sink, not a fall. It
            // dims all the way down to about 40%, then goes out on arrival.
            let scale = (1. - (1. - SMALLEST) * p.powf(0.8)).max(SMALLEST);
            let fade_to = if self.on_heading { 0.1 } else { 0. };
            let opacity = if p < 0.9 { 1. - 0.6 * (p / 0.9) } else { 0.4 - (0.4 - fade_to) * ((p - 0.9) / 0.1) };
            return Frame { x, y, scale, tilt: 0., corner_radius, opacity, dimming: 0.45 * p };
        }
        // Shrinks fastest early on, so it's small well before it arrives.
        let scale = (1. - (1. - SMALLEST) * p.powf(0.7)).max(SMALLEST);
        // A slight tip, most at the middle of the fall.
        let tilt = 4. * (p * std::f32::consts::PI).sin() * std::f32::consts::PI / 180.;
        let fade_from = if self.on_heading { 0.85 } else { 0.6 };
        let fade_to = if self.on_heading { 0.2 } else { 0. };
        let opacity = if p < fade_from { 1. } else { 1. - (p - fade_from) / (1. - fade_from) * (1. - fade_to) };
        Frame { x, y, scale, tilt, corner_radius, opacity, dimming: 0. }
    }
}

/// Where the page aims, given where things are drawn now (window points,
/// downwards). `box_mid` is the heading's box (Archived's, or Hibernating's
/// moon) if it's in sight; `open_row` is the archived thread's row, top and
/// height, if drawn. The row closes up as the page flies, lifting a heading
/// below it by its height, so the page aims where the heading will be.
/// With no box in sight, it aims at the bottom of the list (`list` is its
/// visible left and bottom), just in from its leading edge. None with no list.
pub fn target(box_mid: Option<(f32, f32)>, open_row: Option<(f32, f32)>, list: Option<(f32, f32)>, leading: f32) -> Option<((f32, f32), bool)> {
    if let Some((x, y)) = box_mid {
        let lift = open_row.filter(|(top, _)| *top < y).map_or(0., |(_, height)| height);
        return Some(((x, y - lift), true));
    }
    list.map(|(left, bottom)| ((left + leading, bottom), false))
}

/// How far into a flight the next thread is, for its fade in: 0 hidden,
/// 1 all there. `reduced` is Reduce Motion or a flight with nowhere to go.
pub fn next_thread_shown(elapsed: Duration, to: Destination, reduced: bool) -> f32 {
    let (delay, fade) = if reduced { (REDUCED_NEXT_DELAY, REDUCED_NEXT_FADE) } else { (to.next_thread_delay(), to.next_thread_fade()) };
    let Some(into) = elapsed.checked_sub(delay) else { return 0. };
    let t = (into.as_secs_f32() / fade.as_secs_f32()).min(1.);
    // CAMediaTimingFunction's easeOut, near enough.
    1. - (1. - t) * (1. - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(to: Destination) -> Path {
        Path { start: (600., 400.), end: (40., 700.), swing: to.swing(), on_heading: true, to }
    }

    #[test]
    fn it_starts_on_the_page_and_ends_in_the_box() {
        for to in [Destination::Archive, Destination::Hibernate] {
            let p = path(to);
            let first = p.at_time(0.);
            assert_eq!((first.x, first.y, first.scale, first.opacity), (600., 400., 1., 1.));
            let last = p.at_time(1.);
            assert!((last.x - 40.).abs() < 0.01 && (last.y - 700.).abs() < 0.01, "{to:?} ends at {last:?}");
            assert!((last.scale - SMALLEST).abs() < 0.001);
        }
    }

    #[test]
    fn the_drop_swings_left_and_below_the_heading_before_coming_into_it() {
        let p = path(Destination::Archive);
        // Past the heading's height (downwards) part way, as the Swift path dips under it.
        let lowest = (0..=100).map(|i| p.frame(i as f32 / 100.).y).fold(f32::MIN, f32::max);
        assert!(lowest > 700., "never dipped below the heading: {lowest}");
        let mid = p.frame(0.5);
        assert!(mid.tilt > 0.06 && mid.tilt < 0.071, "tips about 4 degrees mid-fall: {}", mid.tilt);
    }

    #[test]
    fn the_hibernate_stays_level_and_dims() {
        let p = path(Destination::Hibernate);
        let mid = p.frame(0.5);
        assert_eq!(mid.tilt, 0.);
        assert!((mid.dimming - 0.225).abs() < 0.001);
        assert!((p.frame(1.).opacity - 0.1).abs() < 0.001);
    }

    #[test]
    fn off_the_list_it_fades_out_all_the_way() {
        let mut p = path(Destination::Archive);
        p.on_heading = false;
        assert_eq!(p.frame(1.).opacity, 0.);
        assert_eq!(p.frame(0.59).opacity, 1.);
        assert!(p.frame(0.8).opacity < 1.);
    }

    #[test]
    fn the_drop_is_slow_off_the_mark() {
        assert!(progress(0.1, Destination::Archive) < 0.01);
        assert!((progress(0.5, Destination::Archive) - 0.5).abs() < 1e-6);
        assert!(progress(0.9, Destination::Archive) > 0.99);
    }

    #[test]
    fn it_aims_where_the_heading_will_be_once_the_row_has_closed_up() {
        // The row above the heading: the heading moves up by its height.
        assert_eq!(target(Some((20., 500.)), Some((100., 44.)), Some((0., 800.)), 23.), Some(((20., 456.), true)));
        // A row below the box (an archived row already under it) moves nothing.
        assert_eq!(target(Some((20., 500.)), Some((520., 44.)), None, 23.), Some(((20., 500.), true)));
        // No box in sight: the bottom of the list.
        assert_eq!(target(None, Some((100., 44.)), Some((0., 800.)), 23.), Some(((23., 800.), false)));
        assert_eq!(target(None, None, None, 23.), None);
    }

    #[test]
    fn the_next_thread_waits_then_fades_in() {
        let to = Destination::Archive;
        assert_eq!(next_thread_shown(Duration::from_millis(150), to, false), 0.);
        assert!(next_thread_shown(Duration::from_millis(300), to, false) > 0.);
        assert_eq!(next_thread_shown(Duration::from_millis(480), to, false), 1.);
        assert_eq!(next_thread_shown(Duration::from_millis(200), to, true), 1.);
    }
}
