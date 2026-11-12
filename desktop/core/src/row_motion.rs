// Where the sidebar's rows were and where they're going, for their glide when
// the list changes (the Swift app's `Juice.rows` over rows matched in the
// list's space, `sidebarRowSpace`): the app lays a row out where it now is
// and, for a moment, paints it where it was, the difference easing away. A
// row that wasn't there before fades in instead (`Juice.row`). The drawing
// is desktop/gpui/src/row_motion.rs; this is the bookkeeping, with tests.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How long a glide runs at most: `.snappy(duration: 0.32, extraBounce:
/// 0.04)`, which a spring takes a little past its duration to settle.
pub const GLIDE_LIMIT: Duration = Duration::from_millis(600);
/// How long a row that turned up takes to fade in.
pub const ARRIVE: Duration = Duration::from_millis(260);

/// What's left of a glide after `t` seconds, from 1 to 0: a spring settling
/// with a touch of overshoot, as SwiftUI's snappy one whose response is
/// 0.32 s and whose damping is a little under snappy's own.
pub fn spring(t: f32) -> f32 {
    let omega = std::f32::consts::TAU / 0.32;
    let zeta: f32 = 0.81;
    let root = (1. - zeta * zeta).sqrt();
    let damped = omega * root;
    (-zeta * omega * t).exp() * ((damped * t).cos() + zeta / root * (damped * t).sin())
}

/// A list's rows, by key, top to bottom, and where each is.
#[derive(Debug, Default)]
pub struct Glides {
    /// Each row's top in the list as last painted, glide and all.
    tops: HashMap<String, f32>,
    /// Rows whose place changed: where they were, until their new place is
    /// known (it's laid out after this is told).
    starting: HashMap<String, f32>,
    /// Rows gliding: how far from their place they started, and when.
    gliding: HashMap<String, (f32, Instant)>,
    /// Rows that turned up, and when.
    arrivals: HashMap<String, Instant>,
    /// The rows in the order they were last drawn; None before the first.
    last: Option<Vec<String>>,
}

impl Glides {
    /// The rows about to be drawn. Against the last ones, a row somewhere
    /// else is set to glide there and one that wasn't there fades in, unless
    /// it's `quiet` (a section folding moves its rows itself). Nothing moves
    /// on the first rows, nor under Reduce Motion (`reduce`).
    pub fn rows_drawn(&mut self, rows: Vec<String>, quiet: &HashSet<String>, reduce: bool, now: Instant) {
        let Some(before) = self.last.as_ref() else {
            self.last = Some(rows);
            return;
        };
        if *before == rows {
            return;
        }
        let was: HashSet<&str> = before.iter().map(String::as_str).collect();
        for id in &rows {
            if reduce || quiet.contains(id) {
                continue;
            }
            if was.contains(id.as_str()) {
                if let Some(top) = self.tops.get(id) {
                    self.starting.insert(id.clone(), *top);
                }
            } else {
                self.arrivals.insert(id.clone(), now);
            }
        }
        let kept: HashSet<&String> = rows.iter().collect();
        self.tops.retain(|id, _| kept.contains(id));
        self.gliding.retain(|id, _| kept.contains(id));
        self.arrivals.retain(|id, _| kept.contains(id));
        self.last = Some(rows);
    }

    /// A row laid out with its top at `top`: how far from there to paint it
    /// now. Starts its glide if its place has just changed, and notes where
    /// it's painted, so a glide that's overtaken starts from there.
    pub fn place(&mut self, id: &str, top: f32, now: Instant) -> f32 {
        if let Some(from) = self.starting.remove(id) {
            let delta = from - top;
            if delta.abs() >= 0.5 {
                self.gliding.insert(id.to_owned(), (delta, now));
            }
        }
        let offset = match self.gliding.get(id).copied() {
            None => 0.,
            Some((delta, start)) => {
                let elapsed = now.saturating_duration_since(start);
                let left = spring(elapsed.as_secs_f32());
                // Done once it's settled to under a quarter of a point.
                if elapsed >= GLIDE_LIMIT || (elapsed > Duration::from_millis(200) && (left * delta).abs() < 0.25) {
                    self.gliding.remove(id);
                    0.
                } else {
                    delta * left
                }
            }
        };
        self.tops.insert(id.to_owned(), top + offset);
        offset
    }

    /// When the row turned up, while it's still fading in.
    pub fn arrived(&mut self, id: &str, now: Instant) -> Option<Instant> {
        let at = *self.arrivals.get(id)?;
        if now.saturating_duration_since(at) >= ARRIVE {
            self.arrivals.remove(id);
            return None;
        }
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_glide_settles_quickly_with_a_touch_of_overshoot() {
        assert!((spring(0.) - 1.).abs() < 1e-6, "it starts where the row was");
        assert!(spring(0.16) < 0.3, "most of the way there in half the time");
        let lowest = (0..60).map(|i| spring(i as f32 / 100.)).fold(f32::MAX, f32::min);
        assert!(lowest < 0. && lowest > -0.05, "a little past its place, not a bounce: {lowest}");
        assert!(spring(0.5).abs() < 0.01, "settled by half a second");
    }

    #[test]
    fn a_row_somewhere_new_glides_and_a_new_one_fades_in() {
        let now = Instant::now();
        let mut glides = Glides::default();
        glides.rows_drawn(ids(&["a", "b"]), &HashSet::new(), false, now);
        glides.place("a", 0., now);
        glides.place("b", 51., now);
        // b goes to the top and c turns up.
        glides.rows_drawn(ids(&["b", "c", "a"]), &HashSet::new(), false, now);
        assert!(glides.arrived("c", now).is_some());
        assert!(glides.arrived("c", now + ARRIVE).is_none(), "faded in by then");
        // Laid out at the top now, b is painted where it was and eases up.
        assert_eq!(glides.place("b", 0., now), 51.);
        assert!(glides.place("b", 0., now + Duration::from_millis(160)) < 51. * 0.3);
        assert_eq!(glides.place("b", 0., now + GLIDE_LIMIT), 0.);
    }

    #[test]
    fn an_overtaken_glide_starts_from_where_the_row_was_painted() {
        let now = Instant::now();
        let mut glides = Glides::default();
        glides.rows_drawn(ids(&["a", "b"]), &HashSet::new(), false, now);
        glides.place("a", 0., now);
        glides.place("b", 100., now);
        glides.rows_drawn(ids(&["b", "a"]), &HashSet::new(), false, now);
        glides.place("b", 0., now);
        let mid = now + Duration::from_millis(80);
        let painted = glides.place("b", 0., mid);
        // Back where it was, from wherever it had got to.
        glides.rows_drawn(ids(&["a", "b"]), &HashSet::new(), false, mid);
        assert!((glides.place("b", 100., mid) - (painted - 100.)).abs() < 0.01);
    }

    #[test]
    fn nothing_moves_on_the_first_rows_under_reduce_motion_or_in_a_fold() {
        let now = Instant::now();
        let mut glides = Glides::default();
        glides.rows_drawn(ids(&["a"]), &HashSet::new(), false, now);
        assert!(glides.arrived("a", now).is_none(), "the first rows don't fade in");
        glides.place("a", 0., now);
        glides.rows_drawn(ids(&["b", "a"]), &HashSet::new(), true, now);
        assert!(glides.arrived("b", now).is_none());
        assert_eq!(glides.place("a", 51., now), 0.);
        let quiet: HashSet<String> = ["c".to_string()].into();
        glides.rows_drawn(ids(&["b", "a", "c"]), &quiet, false, now);
        assert!(glides.arrived("c", now).is_none(), "a folding section's rows have its own motion");
    }
}
