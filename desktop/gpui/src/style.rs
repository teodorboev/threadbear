// The Swift app's sizes and colours (Transcript/Theme.swift, TypeScale), so the
// two apps look alike and there's one place to change them. Colours take the
// light or dark value from the window's theme.
#![allow(dead_code)] // Filled in ahead of the views that use each value.
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// The window's zoom (TextZoom in Theme.swift): ⌘= and ⌘- scale everything
// together, like a browser's zoom. Every point size in the window goes through
// `px` below, so one number scales them all. A module that draws anything
// does `use crate::style::px;` after its `use gpui_kit::*;`, which shadows
// GPUI's own `px` for that file (a one-line change in each) and leaves a
// raw `gpui_kit::px` only for the rare size that shouldn't scale, like a
// hairline.
static ZOOM_PERCENT: AtomicU32 = AtomicU32::new(100);

pub fn zoom_percent() -> u32 {
    ZOOM_PERCENT.load(Ordering::Relaxed)
}

pub fn set_zoom_percent(percent: u32) {
    ZOOM_PERCENT.store(percent, Ordering::Relaxed);
}

/// `AppAppearance`: light or dark for this app only, or the Mac's (Settings ›
/// General › Appearance). Kept here so the windows' appearance observers can
/// tell whether to follow the Mac.
static APPEARANCE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// The Swift app's raw value, which the settings file keeps.
    pub fn name(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    pub fn from_name(name: &str) -> Appearance {
        match name {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            _ => Appearance::System,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Appearance::System => "Same as the Mac",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }
}

pub fn appearance() -> Appearance {
    match APPEARANCE.load(Ordering::Relaxed) {
        1 => Appearance::Light,
        2 => Appearance::Dark,
        _ => Appearance::System,
    }
}

/// Sets the preference and puts it on the window: the theme is the app's, so
/// every window follows.
pub fn set_appearance(appearance: Appearance, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    APPEARANCE.store(
        match appearance {
            Appearance::System => 0,
            Appearance::Light => 1,
            Appearance::Dark => 2,
        },
        Ordering::Relaxed,
    );
    match appearance {
        Appearance::System => Theme::sync_system_appearance(Some(window), cx),
        Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
    }
}

/// `zoomed`: a design size at the window's zoom (`TypeScale.scaled`).
pub fn zoomed(size: f32) -> f32 {
    threadbear_core::zoom::scaled(size, zoom_percent())
}

/// A SwiftUI `Text`'s own line height at `size`, as SwiftUI lays it out on
/// macOS (measured with NSHostingView.fittingSize: 14 at 11 pt, 16 at 13, 19
/// at 15). GPUI's own is 1.618 times the size, so a view drawn in SwiftUI
/// with no line height of its own comes out this tall per line, not GPUI's.
pub fn natural_line(size: f32) -> f32 {
    match size.round() as i32 {
        ..=11 => 14.,
        12 => 15.,
        13 => 16.,
        14 => 17.,
        15 => 19.,
        _ => (size * 1.22).ceil(),
    }
}

/// GPUI's `px`, at the window's zoom.
pub fn px(size: f32) -> Pixels {
    gpui_kit::px(zoomed(size))
}

/// Text sizes, as TypeScale has them at 100% zoom.
pub mod size {
    pub const BODY: f32 = 15.;
    pub const BODY_LINE: f32 = 23.;
    pub const WORK: f32 = 14.;
    pub const WORK_LINE: f32 = 20.;
    pub const CODE: f32 = 13.;
    pub const CODE_LINE: f32 = 19.5;
    pub const CARD_TITLE: f32 = 14.;
    pub const LABEL: f32 = 13.;
    pub const DETAIL: f32 = 12.;
    pub const SMALL: f32 = 11.;
    pub const SIDEBAR_TITLE: f32 = 14.;
    pub const SIDEBAR_PREVIEW: f32 = 13.;
    pub const SIDEBAR_BUTTON: f32 = 14.;
    pub const SIDEBAR_META: f32 = 11.;
    pub const SIDEBAR_HEADING: f32 = 13.;
    pub const HEADER: f32 = 15.;
    pub const COMPOSER: f32 = 15.;
    pub const CHIP: f32 = 13.;
    /// The send button, and the height of the controls row it sits in.
    pub const SEND_BUTTON: f32 = 28.;
    /// A picture's chip among the words (PictureChip.Metrics at 15 pt).
    pub const CHIP_HEIGHT: f32 = 20.;
    /// A picture in the strip over the words (AttachmentThumbnail.size).
    pub const THUMBNAIL: f32 = 56.;
}

/// Widest the reading column gets; wider windows centre it.
pub const MAX_CONTENT_WIDTH: f32 = 780.;
pub const SIDE_INSET: f32 = 28.;

/// The composer's card (ComposerView).
pub const COMPOSER_RADIUS: f32 = 20.;
pub const COMPOSER_PADDING: f32 = 14.;
pub const COMPOSER_GAP: f32 = 10.;

/// The window's background in each look, as the Swift app draws its panels,
/// sidebar and transcript (NSColor.windowBackgroundColor, BearDock's
/// `DockTheme.panel`). gpui-kit's own dark theme is a near-black #0a0a0a, so the
/// whole GPUI app looked much darker than the Swift one (10 Nov). Put into
/// gpui-kit's light and dark themes before the first switch to either, so
/// every switch after (the system's, the automation's `appearance`) keeps it.
/// Popovers and menus get it lifted a little, as macOS's own menus are on dark.
pub(crate) fn install_window_colours(cx: &mut App) {
    use gpui_kit::component::Theme;
    use std::rc::Rc;
    let (light, dark) = window_backgrounds();
    let hex = |c: Rgba| -> SharedString { format!("#{:02x}{:02x}{:02x}", (c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8).into() };
    let lift = |c: Rgba, by: f32| Rgba { r: c.r + (1. - c.r) * by, g: c.g + (1. - c.g) * by, b: c.b + (1. - c.b) * by, a: 1. };
    let theme = Theme::global_mut(cx);
    for (config, background, popover) in [(&mut theme.light_theme, light, light), (&mut theme.dark_theme, dark, lift(dark, 0.06))] {
        let mut changed = (**config).clone();
        changed.colors.background = Some(hex(background));
        changed.colors.sidebar = Some(hex(background));
        changed.colors.popover = Some(hex(popover));
        *config = Rc::new(changed);
    }
    refresh_system_accent(cx);
    refresh_system_contrast(cx);
    refresh_system_clock();
}

/// Use AppKit's effective accessibility appearance where available, and
/// strengthen its semantic colours for the other look. Cache both palettes.
#[derive(Clone, PartialEq)]
struct SystemContrast {
    enabled: bool,
    revision: u64,
    light: [Hsla; 4],
    dark: [Hsla; 4],
    normal_kit: [[Option<SharedString>; 6]; 2],
}
impl Global for SystemContrast {}

static CLOCK_24H: AtomicBool = AtomicBool::new(false);

/// Cached for wait tips, which draw once per visible row each frame.
pub(crate) fn uses_24_hour_clock() -> bool {
    CLOCK_24H.load(Ordering::Relaxed)
}

/// The Mac's hour cycle, including its 24-hour override. Read on launch,
/// return to the front and opening the picker, never while drawing rows.
pub(crate) fn refresh_system_clock() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSDateFormatter, NSLocale, NSString};
        let pattern = NSDateFormatter::dateFormatFromTemplate_options_locale(&NSString::from_str("j"), 0, Some(&NSLocale::currentLocale()));
        CLOCK_24H.store(pattern.is_some_and(|p| p.to_string().contains(['H', 'k'])), Ordering::Relaxed);
    }
}

pub(crate) type ReadingStyleKey = (bool, u64, u32);

/// A system appearance can replace the contrast colours without changing
/// the app's fixed light/dark look or the Increase Contrast switch.
pub(crate) fn reading_style_key(cx: &App) -> ReadingStyleKey {
    (cx.theme().is_dark(), cx.try_global::<SystemContrast>().map_or(0, |s| s.revision), zoom_percent())
}

pub(crate) fn increase_contrast(cx: &App) -> bool {
    cx.try_global::<SystemContrast>().is_some_and(|s| s.enabled)
}

pub(crate) fn refresh_system_contrast(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let enabled = system_increase_contrast();
        let Some((light, dark)) = system_contrast_colours(false, enabled).zip(system_contrast_colours(true, enabled)) else { return };
        if cx.try_global::<SystemContrast>().is_some_and(|s| s.enabled == enabled && s.light == light && s.dark == dark) { return; }
        let normal_kit = cx.try_global::<SystemContrast>().map(|s| s.normal_kit.clone()).unwrap_or_else(|| {
            let theme = gpui_kit::component::Theme::global(cx);
            [&theme.light_theme, &theme.dark_theme].map(|t| {
                let c = &t.colors;
                [c.foreground.clone(), c.sidebar_foreground.clone(), c.muted_foreground.clone(), c.border.clone(), c.input.clone(), c.sidebar_border.clone()]
            })
        });
        let first = cx.try_global::<SystemContrast>().is_none();
        let revision = cx.try_global::<SystemContrast>().map_or(1, |s| s.revision.wrapping_add(1));
        cx.set_global(SystemContrast { enabled, revision, light, dark, normal_kit: normal_kit.clone() });
        if first && !enabled { return; }
        // Kit controls (inputs, calendars and menus) use these tokens rather
        // than the transcript's style functions. Resolve both from AppKit.
        use gpui_kit::component::Theme;
        use std::rc::Rc;
        let theme = Theme::global_mut(cx);
        let mode = theme.mode;
        let hex = |c: Hsla| -> SharedString {
            let c = Rgba::from(c);
            format!("#{:02x}{:02x}{:02x}{:02x}", (c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8, (c.a * 255.).round() as u8).into()
        };
        for ((config, colours), normal) in [(&mut theme.light_theme, light), (&mut theme.dark_theme, dark)].into_iter().zip(normal_kit) {
            let mut changed = (**config).clone();
            let [foreground, sidebar_foreground, muted_foreground, border, input, sidebar_border] = if enabled {
                [0, 0, 1, 3, 3, 3].map(|i| Some(hex(colours[i])))
            } else { normal };
            changed.colors.foreground = foreground;
            changed.colors.sidebar_foreground = sidebar_foreground;
            changed.colors.muted_foreground = muted_foreground;
            changed.colors.border = border;
            changed.colors.input = input;
            changed.colors.sidebar_border = sidebar_border;
            *config = Rc::new(changed);
        }
        Theme::change(mode, None, cx);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

#[cfg(target_os = "macos")]
fn system_increase_contrast() -> bool {
    // A scripted app can show the two looks without changing Mike's setting.
    #[cfg(feature = "automation")]
    match std::env::var("THREADBEAR_INCREASE_CONTRAST").as_deref() {
        Ok("on") => return true,
        Ok("off") => return false,
        _ => {}
    }
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn system_contrast_colours(dark: bool, contrast: bool) -> Option<[Hsla; 4]> {
    use objc2_app_kit::{NSApplication, NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua, NSColor, NSColorSpace};
    use objc2::MainThreadMarker;
    use objc2_foundation::NSArray;
    unsafe {
        let name = if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua };
        let mut appearance = NSAppearance::appearanceNamed(name)?;
        // High-contrast names are for matching only. Apple's SDK explicitly
        // forbids passing them to appearanceNamed (older macOS returns nil).
        let native = contrast && MainThreadMarker::new().is_some_and(|mtm| {
            let effective = NSApplication::sharedApplication(mtm).effectiveAppearance();
            let names = NSArray::from_slice(&[NSAppearanceNameAccessibilityHighContrastDarkAqua, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameDarkAqua, NSAppearanceNameAqua]);
            let wanted = if dark { NSAppearanceNameAccessibilityHighContrastDarkAqua } else { NSAppearanceNameAccessibilityHighContrastAqua };
            if effective.bestMatchFromAppearancesWithNames(&names).is_some_and(|n| &*n == wanted) {
                appearance = effective;
                true
            } else { false }
        });
        let previous = NSAppearance::currentAppearance();
        NSAppearance::setCurrentAppearance(Some(&appearance));
        let resolve = |color: objc2::rc::Retained<NSColor>| -> Option<Hsla> {
            let c = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
            Some(srgb(c.redComponent() as f32, c.greenComponent() as f32, c.blueComponent() as f32, c.alphaComponent() as f32))
        };
        let colours = [NSColor::labelColor(), NSColor::secondaryLabelColor(), NSColor::tertiaryLabelColor(), NSColor::separatorColor()].map(resolve);
        NSAppearance::setCurrentAppearance(previous.as_deref());
        let mut colours = [colours[0]?, colours[1]?, colours[2]?, colours[3]?];
        if contrast && !native {
            // The system only exposes its current effective look. Keep the
            // other look, and automation's preview, readable without changing
            // system preferences or constructing an unsupported appearance.
            for (colour, minimum) in colours.iter_mut().zip([0.95, 0.75, 0.6, 0.5]) {
                colour.a = colour.a.max(minimum);
            }
        }
        Some(colours)
    }
}

fn contrast_colour(cx: &App, index: usize, fallback: Hsla) -> Hsla {
    match cx.try_global::<SystemContrast>().filter(|s| s.enabled) {
        Some(s) => if cx.theme().is_dark() { s.dark[index] } else { s.light[index] },
        None => fallback,
    }
}

#[derive(Clone, Copy, PartialEq)]
struct SystemAccent { light: Rgba, dark: Rgba }
impl Global for SystemAccent {}

/// System Settings can change the accent while this app is behind it. Read
/// it at launch and on return, once for both appearances, rather than ask
/// AppKit for every button in every frame.
pub(crate) fn refresh_system_accent(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        use gpui_kit::component::Theme;
        use std::rc::Rc;
        let Some((light, dark)) = system_control_accent(false).zip(system_control_accent(true)) else { return };
        let accent = SystemAccent { light, dark };
        if cx.try_global::<SystemAccent>() == Some(&accent) { return; }
        cx.set_global(accent);
        let theme = Theme::global_mut(cx);
        let mode = theme.mode;
        for (config, color) in [(&mut theme.light_theme, light), (&mut theme.dark_theme, dark)] {
            let hex: SharedString = format!("#{:02x}{:02x}{:02x}", (color.r * 255.).round() as u8, (color.g * 255.).round() as u8, (color.b * 255.).round() as u8).into();
            let mut changed = (**config).clone();
            changed.colors.primary = Some(hex.clone());
            changed.colors.ring = Some(hex);
            let shade = |toward: f32| -> SharedString {
                let mix = |c: f32| ((c * 0.9 + toward * 0.1) * 255.).round() as u8;
                format!("#{:02x}{:02x}{:02x}", mix(color.r), mix(color.g), mix(color.b)).into()
            };
            changed.colors.primary_hover = Some(shade(1.));
            changed.colors.primary_active = Some(shade(0.));
            *config = Rc::new(changed);
        }
        Theme::change(mode, None, cx);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn system_control_accent(dark: bool) -> Option<Rgba> {
    use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace};
    unsafe {
        let name = if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua };
        let appearance = NSAppearance::appearanceNamed(name)?;
        let previous = NSAppearance::currentAppearance();
        NSAppearance::setCurrentAppearance(Some(&appearance));
        let color = NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
        NSAppearance::setCurrentAppearance(previous.as_deref());
        let color = color?;
        Some(srgb_rgba(color.redComponent() as f32, color.greenComponent() as f32, color.blueComponent() as f32))
    }
}

/// NSColor.windowBackgroundColor in the light and dark looks. On the Mac it's
/// read from AppKit, so it follows whatever this macOS draws; elsewhere it's
/// what macOS 26 and 27 give, so the app looks the same everywhere.
fn window_backgrounds() -> (Rgba, Rgba) {
    let fallback = (srgb_rgba(1., 1., 1.), srgb_rgba(30. / 255., 30. / 255., 30. / 255.));
    #[cfg(target_os = "macos")]
    {
        let light = system_window_background(false).unwrap_or(fallback.0);
        let dark = system_window_background(true).unwrap_or(fallback.1);
        (light, dark)
    }
    #[cfg(not(target_os = "macos"))]
    fallback
}

fn srgb_rgba(r: f32, g: f32, b: f32) -> Rgba {
    Rgba { r, g, b, a: 1. }
}

// A dynamic colour resolves for the current appearance. The block form
// (performAsCurrentDrawingAppearance) needs block2; setting it is deprecated
// but does the same for a moment on the main thread.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn system_window_background(dark: bool) -> Option<Rgba> {
    use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace};
    unsafe {
        let name = if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua };
        let appearance = NSAppearance::appearanceNamed(name)?;
        let previous = NSAppearance::currentAppearance();
        NSAppearance::setCurrentAppearance(Some(&appearance));
        let color = NSColor::windowBackgroundColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
        NSAppearance::setCurrentAppearance(previous.as_deref());
        let c = color?;
        Some(srgb_rgba(c.redComponent() as f32, c.greenComponent() as f32, c.blueComponent() as f32))
    }
}

fn pick(cx: &App, dark: Hsla, light: Hsla) -> Hsla {
    if cx.theme().is_dark() {
        dark
    } else {
        light
    }
}

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

fn black(alpha: f32) -> Hsla {
    hsla(0., 0., 0., alpha)
}

pub fn card_background(cx: &App) -> Hsla {
    pick(cx, white(0.035), black(0.02))
}

pub fn card_border(cx: &App) -> Hsla {
    contrast_colour(cx, 3, pick(cx, white(0.12), black(0.12)))
}

pub fn code_background(cx: &App) -> Hsla {
    pick(cx, white(0.045), black(0.03))
}

pub fn inline_code_background(cx: &App) -> Hsla {
    pick(cx, white(0.085), black(0.05))
}

pub fn hover_fill(cx: &App) -> Hsla {
    pick(cx, white(0.05), black(0.04))
}

pub fn hairline(cx: &App) -> Hsla {
    contrast_colour(cx, 3, pick(cx, white(0.1), black(0.1)))
}

pub fn faint_text(cx: &App) -> Hsla {
    contrast_colour(cx, 1, pick(cx, white(0.36), black(0.45)))
}

pub fn quiet_text(cx: &App) -> Hsla {
    contrast_colour(cx, 1, pick(cx, white(0.55), black(0.6)))
}

pub fn button_fill(cx: &App) -> Hsla {
    pick(cx, white(0.12), white(1.))
}

pub fn button_edge(cx: &App) -> Hsla {
    contrast_colour(cx, 3, pick(cx, white(0.08), black(0.18)))
}

/// The composer's card: a lighter fill on dark, white on light (ComposerView.card).
pub fn composer_fill(cx: &App) -> Hsla {
    pick(cx, white(0.085), white(1.))
}

/// The empty look of the send button (CircleButtonStyle, not filled).
pub fn send_faint_fill(cx: &App) -> Hsla {
    pick(cx, white(0.1), black(0.1))
}

/// A picture chip's fill (PictureChip.fill).
pub fn chip_fill(cx: &App) -> Hsla {
    pick(cx, white(0.1), black(0.06))
}

/// A composer chip's hover fill (ComposerChip).
pub fn chip_hover_fill(cx: &App) -> Hsla {
    pick(cx, white(0.07), black(0.05))
}

/// The hairline between chips (ChipDivider).
pub fn chip_divider(cx: &App) -> Hsla {
    contrast_colour(cx, 3, pick(cx, white(0.12), black(0.12)))
}

/// The Needs you orange (Theme.needsYou): systemOrange on dark, a deeper
/// orange on light, the same as `needs_you_text`.
pub fn needs_you(cx: &App) -> Hsla {
    needs_you_text(cx)
}

/// The answer box under an ask's Something else.
pub fn answer_box_fill(cx: &App) -> Hsla {
    pick(cx, white(0.05), black(0.03))
}

/// The system's accent (NSColor.controlAccentColor): the outline of a box
/// that needs Mike, ticks and the agent's own pick.
pub fn accent(cx: &App) -> Hsla {
    pick(cx, rgb_hsla(0x0a84ff), rgb_hsla(0x007aff))
}

fn rgb_hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

// The transcript's colours beyond the cards' (Theme.swift).

/// `color` drawn over the window's background, for text runs: GPUI blends a
/// highlight's colour into the run's default colour, so a translucent one
/// (every transcript colour here) would come out as the default colour.
pub fn over_background(color: Hsla, cx: &App) -> Hsla {
    let (bg, c) = (Rgba::from(cx.theme().background), Rgba::from(color));
    Hsla::from(Rgba { r: bg.r * (1. - c.a) + c.r * c.a, g: bg.g * (1. - c.a) + c.g * c.a, b: bg.b * (1. - c.a) + c.b * c.a, a: 1. })
}

/// NSColor.labelColor: body text.
pub fn label_text(cx: &App) -> Hsla {
    contrast_colour(cx, 0, pick(cx, white(0.85), black(0.85)))
}

/// NSColor.secondaryLabelColor: a table's headers, quoted text.
pub fn secondary_label(cx: &App) -> Hsla {
    contrast_colour(cx, 1, pick(cx, white(0.55), black(0.5)))
}

/// NSColor.tertiaryLabelColor, and `Theme.faintIcon`'s dark value: work line
/// icons and chevrons, the "⏎" on the Allow button.
pub fn tertiary_label(cx: &App) -> Hsla {
    contrast_colour(cx, 2, pick(cx, white(0.25), black(0.25)))
}

/// `Theme.faintIcon`: work line icons and chevrons.
pub fn faint_icon(cx: &App) -> Hsla {
    contrast_colour(cx, 2, pick(cx, white(0.25), black(0.36)))
}

/// The pane's row icons (`Theme.faintIcon`): a notch brighter here, as
/// Lucide's thin strokes read fainter than SF Symbols.
pub fn pane_icon(cx: &App) -> Hsla {
    contrast_colour(cx, 1, pick(cx, white(0.4), black(0.42)))
}

/// `Theme.answerRowsFill`.
pub fn answer_rows_fill(cx: &App) -> Hsla {
    pick(cx, white(0.02), black(0.015))
}

/// The small star beside "Recommended" on the agent's own pick: a muted gold,
/// as on the Mac app (Theme.recommendedStar), after Ghostex's (Mike, 9 Nov 2026).
pub fn recommended_star(cx: &App) -> Hsla {
    pick(cx, srgb(0.86, 0.70, 0.33, 1.), srgb(0.72, 0.54, 0.12, 1.))
}

fn srgb(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Hsla::from(Rgba { r, g, b, a })
}

/// NSColor.controlAccentColor on a Mac, the existing blue elsewhere.
pub fn control_accent(cx: &App) -> Hsla {
    if let Some(accent) = cx.try_global::<SystemAccent>() {
        return Hsla::from(if cx.theme().is_dark() { accent.dark } else { accent.light });
    }
    pick(cx, srgb(0.04, 0.52, 1.0, 1.0), srgb(0.0, 0.48, 1.0, 1.0))
}

/// NSColor.linkColor, as macOS 27 gives it in sRGB: a lighter blue than the
/// accent in the dark, a deeper one in the light (measured from the Swift app's
/// own colour, 12 Nov; this was the accent's blue before and read deeper than
/// the Swift app's links).
pub fn link_text(cx: &App) -> Hsla {
    pick(cx, srgb(0.255, 0.612, 1.0, 1.0), srgb(0.0, 0.408, 0.855, 1.0))
}

/// A little of the accent over the faint fill behind Mike's own messages.
pub fn bubble_background(cx: &App) -> Hsla {
    let dark = cx.theme().is_dark();
    let accent = Rgba::from(control_accent(cx));
    let (base, base_alpha, accent_alpha): (f32, f32, f32) = if dark { (1., 0.06, 0.09) } else { (0., 0.035, 0.09) };
    let alpha = accent_alpha + base_alpha * (1. - accent_alpha);
    let mix = |c: f32| (c * accent_alpha + base * base_alpha * (1. - accent_alpha)) / alpha;
    srgb(mix(accent.r), mix(accent.g), mix(accent.b), alpha)
}

/// A faint line's tint in a diff: green for added lines, red for removed.
pub fn added_line_bg(cx: &App) -> Hsla {
    pick(cx, srgb(0.25, 0.78, 0.4, 0.12), srgb(0.1, 0.62, 0.25, 0.1))
}

pub fn removed_line_bg(cx: &App) -> Hsla {
    pick(cx, srgb(0.95, 0.3, 0.3, 0.12), srgb(0.85, 0.2, 0.2, 0.09))
}

pub fn added_text(cx: &App) -> Hsla {
    pick(cx, srgb(0.42, 0.84, 0.5, 1.), srgb(0.1, 0.55, 0.22, 1.))
}

pub fn removed_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.45, 0.45, 1.), srgb(0.8, 0.15, 0.15, 1.))
}

/// `Theme.failed`: "Failed" and a failed tool's icon.
pub fn failed_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.27, 0.23, 1.), srgb(0.78, 0.13, 0.13, 1.))
}

/// `Theme.needsYou` and NSColor.systemOrange.
pub fn needs_you_text(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.62, 0.04, 1.), srgb(0.7, 0.33, 0., 1.))
}

/// `Theme.errorBackground`: behind a failed turn's message.
pub fn error_background(cx: &App) -> Hsla {
    pick(cx, srgb(0.7, 0.2, 0.2, 0.18), srgb(1., 0.94, 0.94, 1.))
}

// The sidebar's colours (SidebarMetrics, ThreadRowView, ProjectTiles).

/// `Color.primary` at an opacity: white in dark mode, black in light.
pub fn primary_opacity(cx: &App, alpha: f32) -> Hsla {
    pick(cx, white(alpha), black(alpha))
}

/// A row under the pointer (SidebarMetrics.hover).
pub fn sidebar_hover(cx: &App) -> Hsla {
    primary_opacity(cx, 0.03)
}

/// The open thread's row, brighter than a hover so the two read apart.
pub fn sidebar_selected(cx: &App) -> Hsla {
    sidebar_selection(cx, true)
}

pub fn sidebar_selection(cx: &App, active: bool) -> Hsla {
    primary_opacity(cx, if active { 0.08 } else { 0.06 })
}

/// Theme.failed.
pub fn failed(cx: &App) -> Hsla {
    pick(cx, rgb(0xff453a).into(), hsla(0.0, 0.71, 0.46, 1.0))
}

/// `.secondary` text (secondaryLabelColor).
pub fn secondary_text(cx: &App) -> Hsla {
    contrast_colour(cx, 1, pick(cx, white(0.55), black(0.5)))
}

/// `SidebarMetrics.corner`: the rails' lit square.
pub const RAIL_CORNER: f32 = 8.;

/// The connection banner's warning fill (`.yellow.opacity(0.12)`).
pub fn warning_fill(cx: &App) -> Hsla {
    pick(cx, srgb(1., 0.8, 0., 0.12), srgb(1., 0.8, 0., 0.12))
}

/// The tick on the end-of-inbox note (`Color.green`).
pub fn tick_green(cx: &App) -> Hsla {
    pick(cx, srgb(0.2, 0.83, 0.4, 1.), srgb(0.2, 0.7, 0.3, 1.))
}

/// The app's own menus that are popovers rather than the kit's PopupMenu
/// (the composer's + and permission pickers, the Move sheet's Login, the
/// action chips' "+N"): they're menus to the person using them, so they
/// look like every other menu (the kit's menu/mac_look.rs, the look Mike
/// picked on 12 Nov). Not zoomed, as macOS's menus aren't.
pub mod menu {
    use gpui_kit::component::menu::mac_look;
    use gpui_kit::component::popover::Popover;
    use gpui_kit::component::{h_flex, Icon, Sizable as _};
    use gpui_kit::prelude::*;
    use gpui_kit::{div, px, App, Div, ElementId, FontWeight, Stateful};

    pub use mac_look::colors;

    /// The popover's frame: the menu's fill, edge, inner rim, corners and
    /// shadow, and its padding round the rows.
    pub fn popover(popover: Popover, cx: &App) -> Popover {
        let look = colors(cx);
        popover
            .bg(look.background)
            .text_color(look.label)
            .border_1()
            .border_color(look.edge)
            .rounded(mac_look::RADIUS)
            .shadow(mac_look::shadow(cx))
            .p(mac_look::PADDING)
            .relative()
            // The light line just inside the edge, over the rows and not in
            // the way of the pointer (it has no handlers).
            .child(div().absolute().top_0().left_0().right_0().bottom_0().rounded(mac_look::RADIUS - px(1.)).border_1().border_color(look.rim))
    }

    /// The left edge of a row's icon and of its title, from the row's left, as
    /// the kit's menus have them with an icon column and no ticks.
    const ICON_X: f32 = 12.;
    const TITLE_X: f32 = 36.;

    /// A line: its icon in the icon colour, its title, and the wash under the
    /// pointer, with the title brightened on it. With no icon the title starts
    /// where the icon would, as the kit's menus with no icons have it. The caller adds the title
    /// (and whatever's beside it) and the click.
    pub fn row(id: impl Into<ElementId>, icon: Option<Icon>, cx: &App) -> Stateful<Div> {
        let look = colors(cx);
        h_flex()
            .id(id.into())
            .min_h(mac_look::ROW_HEIGHT)
            .py(px(3.))
            .pl(px(ICON_X))
            .pr(px(12.))
            .gap(px(TITLE_X - ICON_X - f32::from(mac_look::ICON_SIZE)))
            .rounded(mac_look::ROW_RADIUS)
            .text_size(mac_look::TEXT_SIZE)
            .font_weight(FontWeight::NORMAL)
            .cursor_pointer()
            .hover(move |s| s.bg(look.highlight).text_color(look.highlight_label))
            .when_some(icon, |s, icon| s.child(icon.with_size(mac_look::ICON_SIZE).text_color(look.icon)))
    }

    /// The tick at a row's right end, for the current choice.
    pub fn tick(cx: &App) -> Icon {
        Icon::from(gpui_kit::assets::IconName::Check).with_size(px(13.)).text_color(colors(cx).label)
    }

    /// A line between groups of rows, as the menus' separator.
    pub fn separator(cx: &App) -> Div {
        div().py(px(5.)).px(mac_look::SEPARATOR_INSET).child(div().h(px(1.)).bg(colors(cx).separator))
    }

    /// A group's heading, in line with the titles under it.
    pub fn heading(title: &'static str, cx: &App) -> Div {
        div().pl(px(TITLE_X)).pt(px(6.)).pb(px(2.)).text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(colors(cx).icon).child(title)
    }
}

/// A text colour (or an icon's: gpui-kit's `Icon` takes the text colour)
/// that changes while the pointer is over `group`'s element, as the Swift
/// app's `.foregroundStyle(hovering ? ...)` does. Use this, never a bare
/// `.group_hover(g, |s| s.text_color(c))` (`group_hover_text_tests` fails on
/// one): GPUI shapes text, colour and all, while laying out, but an element
/// with no id only learns its group is hovered while painting, after that,
/// so the colour never reached the words. The sidebar's section headings
/// stayed grey under the pointer that way (12 Nov). An id gives the element
/// state to keep the hover in: it hears the pointer arrive, then lays out
/// again with it. Hence `StatefulInteractiveElement`, which only an element
/// with an id is.
pub fn group_hover_text<E: StatefulInteractiveElement>(el: E, group: impl Into<SharedString>, color: Hsla) -> E {
    el.group_hover(group, move |s| s.text_color(color))
}

/// In a window of the test platform's (the kit's test support comes with the
/// `automation` feature, as reply_links.rs's probe tests say).
#[cfg(all(test, feature = "automation"))]
mod group_hover_text_tests {
    use super::group_hover_text;
    use gpui_kit::prelude::*;
    use gpui_kit::test::TestEventExt as _;
    use gpui_kit::{div, point, px, size, AnyElement, App, Bounds, ElementId, GlobalElementId, Hsla, InspectorElementId, LayoutId, Pixels, Point, TestAppContext, Window, WindowBounds, WindowOptions};
    use std::cell::Cell;
    use std::rc::Rc;

    /// Notes the text colour in force while laying out, which is the one
    /// text is shaped in (and an `Icon` is drawn in).
    struct LayoutColour(Rc<Cell<Option<Hsla>>>);

    impl IntoElement for LayoutColour {
        type Element = Self;
        fn into_element(self) -> Self {
            self
        }
    }

    impl Element for LayoutColour {
        type RequestLayoutState = ();
        type PrepaintState = ();
        fn id(&self) -> Option<ElementId> {
            None
        }
        fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
            None
        }
        fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
            self.0.set(Some(window.text_style().color));
            (window.request_layout(gpui_kit::Style { size: size(px(40.).into(), px(12.).into()), ..Default::default() }, [], cx), ())
        }
        fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut Window, _: &mut App) {}
        fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), _: &mut Window, _: &mut App) {}
    }

    const QUIET: Hsla = Hsla { h: 0., s: 0., l: 0.5, a: 1. };
    const BRIGHT: Hsla = Hsla { h: 0., s: 0., l: 1., a: 1. };

    /// A heading as the sidebar draws one: a group with the title in it.
    struct Heading(Rc<Cell<Option<Hsla>>>);

    impl Render for Heading {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let title: AnyElement = group_hover_text(div().id("title").text_color(QUIET), "heading", BRIGHT).child(LayoutColour(self.0.clone())).into_any_element();
            div().size_full().child(div().group("heading").w(px(200.)).h(px(30.)).child(title))
        }
    }

    #[gpui_kit::test]
    fn a_hovered_group_brightens_its_text(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let seen = Rc::new(Cell::new(None));
        let in_view = seen.clone();
        let window = cx.update(|cx| {
            let bounds = Bounds { origin: Point::default(), size: size(px(300.), px(100.)) };
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
            gpui_kit::open_window(options, cx, move |_, cx| cx.new(|_| Heading(in_view))).expect("a test window").0
        });
        cx.update_window(window, |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            assert_eq!(seen.get(), Some(QUIET), "away from the pointer");
            // On the group's edge, not over the title.
            window.pointer_move(point(px(180.), px(15.)), None, cx);
            window.render_frame(cx);
            assert_eq!(seen.get(), Some(BRIGHT), "under the pointer, as the text is laid out");
            window.pointer_move(point(px(250.), px(80.)), None, cx);
            window.render_frame(cx);
            assert_eq!(seen.get(), Some(QUIET), "and back once it's gone");
        })
        .expect("the test window");
    }

}

#[cfg(test)]
mod group_hover_text_source_tests {
    /// Every text colour that follows a group goes through `group_hover_text`.
    #[test]
    fn no_bare_group_hover_text_colours() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut bad = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("style.rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (n, line) in text.lines().enumerate() {
                        if let Some(at) = line.find(".group_hover(") {
                            if line[at..].contains("text_color") {
                                bad.push(format!("{}:{}", path.display(), n + 1));
                            }
                        }
                    }
                }
            }
        }
        assert!(bad.is_empty(), "use style::group_hover_text, so the colour reaches the text:\n{}", bad.join("\n"));
    }
}

#[cfg(all(test, feature = "automation"))]
mod contrast_tests {
    use super::{label_text, quiet_text, faint_text, hairline, sidebar_selection, SystemContrast, black};
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn contrast_changes_reading_colours_and_restores_the_regular_palette(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let regular = (label_text(cx), quiet_text(cx), faint_text(cx), hairline(cx));
            let colours = [black(1.), black(0.85), black(0.65), black(0.6)];
            cx.set_global(SystemContrast { enabled: true, revision: 1, light: colours, dark: colours, normal_kit: Default::default() });
            assert_eq!(label_text(cx), colours[0]);
            assert_eq!(quiet_text(cx), colours[1]);
            assert_eq!(faint_text(cx), colours[1]);
            assert_eq!(hairline(cx), colours[3]);
            let key = super::reading_style_key(cx);
            cx.global_mut::<SystemContrast>().revision += 1;
            assert_ne!(super::reading_style_key(cx), key, "a new palette invalidates cached text even with the same look and contrast flag");
            cx.global_mut::<SystemContrast>().enabled = false;
            assert_eq!((label_text(cx), quiet_text(cx), faint_text(cx), hairline(cx)), regular);
            assert_eq!(sidebar_selection(cx, true).a, 0.08);
            assert_eq!(sidebar_selection(cx, false).a, 0.06);
        });
    }
}

#[cfg(all(test, target_os = "macos"))]
mod native_contrast_tests {
    #[test]
    fn native_palette_resolves_both_looks_with_contrast_requested() {
        for dark in [false, true] {
            let colours = super::system_contrast_colours(dark, true).expect("supported AppKit appearance and semantic colours");
            assert!(colours.iter().all(|c| c.a > 0.));
        }
    }
}
