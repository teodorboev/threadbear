//! Threadbear's change: how every menu in the app looks.
//!
//! The layout is macOS's own (a tick column, then the icon, then the title),
//! measured off real menus (NSMenu and SwiftUI's) captured from the screen on
//! macOS 26, 12 Nov 2026 (features/tools/menu-shot.ts,
//! docs/gpui-port/PARITY.md), in points. The look on top of it is softer
//! than the system's: Mike picked it on 12 Nov from three variants ("C",
//! ~/Library/Caches/Threadbear/gpui-compare/menu-options). A faint wash for
//! the line under the pointer instead of the system's blue, a size taller
//! rows, rounder corners, quieter edges, and icons a size up in their own
//! dimmer colour so the titles lead.
//!
//! Public so the app's own menu-like popovers (the composer's + and
//! permission pickers) can draw themselves the same (style.rs there).

use crate::ActiveTheme as _;
use gpui::{App, BoxShadow, Hsla, Pixels, hsla, point, px, rgb};

/// A line's height: rows follow each other with no gap.
pub const ROW_HEIGHT: Pixels = px(26.);
/// The title's size, the system font's at its menu size.
pub const TEXT_SIZE: Pixels = px(13.);
/// The menu's corners, at its outer edge.
pub const RADIUS: Pixels = px(16.);
/// The highlight's corners.
pub const ROW_RADIUS: Pixels = px(10.);
/// From the inside of the menu's 1 point edge to the rows, on every side:
/// the highlight stands 6 points in from the outer edge.
pub const PADDING: Pixels = px(6.);
/// A separator's band, its line in the middle, 11 points in from each end of
/// a row (17 from the menu's outer edge).
pub const SEPARATOR_HEIGHT: Pixels = px(11.);
pub const SEPARATOR_INSET: Pixels = px(12.);

/// Where a line's pieces start, from the row's left, as drawn ink: the tick,
/// the icon and the title. A menu with no ticked line has no tick column and
/// one with no icons no icon column, so the title moves left as macOS's does.
#[derive(Clone, Copy)]
pub(crate) struct Columns {
    pub check: Option<f32>,
    pub icon: Option<f32>,
    pub title: f32,
}

pub(crate) fn columns(ticks: bool, icons: bool) -> Columns {
    match (ticks, icons) {
        (false, false) => Columns { check: None, icon: None, title: 12. },
        (true, false) => Columns { check: Some(5.), icon: None, title: 19. },
        (false, true) => Columns { check: None, icon: Some(13.), title: 36. },
        (true, true) => Columns { check: Some(5.), icon: Some(22.), title: 45. },
    }
}

/// The title's ink to the row's right end, and to the right of a key or the
/// submenu's chevron: 18 points to the menu's outer edge.
pub(crate) const TRAILING: f32 = 12.;
/// Between a title and its key or chevron, at the least.
pub(crate) const KEY_GAP: f32 = 18.;

/// How far the ink of what's drawn starts inside its box, so the columns
/// above can be given as ink. The title's first letter has about a point of
/// side bearing at 13; the kit's Lucide icons are drawn 2 points in from
/// their box at 12, and the tick below about 1.
pub(crate) const TITLE_BEARING: f32 = 1.;
pub(crate) const ICON_BEARING: f32 = 1.;
pub(crate) const CHECK_BEARING: f32 = 1.;
/// Icons at 15 points, a size up on SF Symbols' ink at 13, so they read
/// beside the titles in their dimmer colour.
pub const ICON_SIZE: Pixels = px(15.);
/// The tick stays at 12, about the ink SF's has at 13: at 15 it crowds the
/// title in a menu with ticks and no icons (the sort menu).
pub(crate) const CHECK_SIZE: Pixels = px(12.);
/// The icons' outline, of the 24 a Lucide icon's box has (Lucide draws 2).
pub(crate) const ICON_STROKE: f32 = 2.;
/// The submenu's chevron: 6 by 10 of ink, its right end where a title's is.
pub(crate) const CHEVRON_SIZE: Pixels = px(17.);
pub(crate) const CHEVRON_BEARING: f32 = 5.67;

/// SF's `checkmark` at a menu's weight: a short stroke and a long one rising
/// well above it, 9 by 9 at 12 points. Lucide's check is wide and low.
pub(crate) const CHECKMARK: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M4 13.5l5 5.5L20 3.5"/></svg>"#;

/// The menu's colours in the current look.
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Hsla,
    /// The 1 point line round the outside.
    pub edge: Hsla,
    /// The light line just inside it: 1 point on dark, 2 on light.
    pub rim: Hsla,
    pub rim_width: Pixels,
    pub label: Hsla,
    pub disabled: Hsla,
    /// Keys and other second things on a line (a submenu's chevron is the title's colour).
    pub secondary: Hsla,
    pub separator: Hsla,
    pub highlight: Hsla,
    pub highlight_label: Hsla,
    /// The icons: dimmer than the titles, so the words lead.
    pub icon: Hsla,
}

pub fn colors(cx: &App) -> Colors {
    let white = |a: f32| hsla(0., 0., 1., a);
    let black = |a: f32| hsla(0., 0., 0., a);
    if cx.theme().is_dark() {
        Colors {
            background: rgb(0x2b2b2d).into(),
            edge: black(0.6),
            rim: white(0.12),
            rim_width: px(1.),
            label: white(0.9),
            disabled: white(0.25),
            secondary: white(0.25),
            separator: white(0.08),
            // A faint wash rather than the system's blue.
            highlight: white(0.1),
            highlight_label: white(1.),
            icon: white(0.6),
        }
    } else {
        Colors {
            background: rgb(0xfafafa).into(),
            // Softer than the system's, as on dark.
            edge: black(0.12),
            rim: white(0.8),
            rim_width: px(1.),
            label: black(0.85),
            disabled: black(0.25),
            secondary: black(0.25),
            separator: black(0.07),
            highlight: black(0.06),
            highlight_label: black(0.9),
            icon: black(0.55),
        }
    }
}

/// The soft shadow under it: wide on light, where it's what shows the edge,
/// and darker on dark.
pub fn shadow(cx: &App) -> Vec<BoxShadow> {
    let alpha = if cx.theme().is_dark() { 0.4 } else { 0.14 };
    vec![BoxShadow { color: hsla(0., 0., 0., alpha), offset: point(px(0.), px(10.)), blur_radius: px(30.), spread_radius: px(0.), inset: false }]
}
