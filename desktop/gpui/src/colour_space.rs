// Every window's drawing tagged as sRGB, so the screen shows the colours the
// app asks for.
//
// The app's colours are sRGB values, as the Swift app's are (style.rs's
// `srgb`, NSColor read in the sRGB space, SwiftUI's `Color(red:green:blue:)`).
// GPUI draws into a CAMetalLayer and only sets its colour space when a window
// asks for Display P3. A layer with none has its pixels sent to the screen
// unconverted, which on a wide-gamut screen makes every colour more saturated
// than the Swift app's. macOS 27 already gives a new CAMetalLayer sRGB (checked
// on 12 Nov on the M5 and Tinker Desk: the app's colours reached the screen
// the same with and without this, features/tools/colour-shot.ts), so this
// says it outright rather than leaning on that default.
//
// Done from a new root view (every window the app opens has one, the dock's
// own included): the platform window and its layer exist by then, and nothing
// has been drawn. The overlay layers GPUI adds later over native views (web
// pages, videos) copy the base layer's colour space when they're made.

use gpui_kit::{App, Window};

/// Tags each new window's layer from now on.
pub(crate) fn install(cx: &mut App) {
    cx.observe_new::<gpui_kit::base::Root>(|_, window, _| {
        if let Some(window) = window {
            tag_srgb(window);
        }
    })
    .detach();
}

#[cfg(target_os = "macos")]
fn tag_srgb(window: &Window) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_graphics::{kCGColorSpaceSRGB, CGColorSpace};
    use objc2_quartz_core::CAMetalLayer;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // SAFETY: the window's own NSView, alive as long as the window is, asked
    // for its backing layer on the main thread.
    let view: &AnyObject = unsafe { handle.ns_view.cast::<AnyObject>().as_ref() };
    let layer: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![view, layer] };
    let Some(Ok(layer)) = layer.map(|l| l.downcast::<CAMetalLayer>()) else { return };
    // One the window chose itself (GPUI's Display P3 option) stays.
    if layer.colorspace().is_some() {
        return;
    }
    // SAFETY: an extern constant from Core Graphics.
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
    layer.setColorspace(srgb.as_deref());
}

#[cfg(not(target_os = "macos"))]
fn tag_srgb(_window: &Window) {}

/// The main window's layer's colour space, for the dump: "sRGB", "none" or
/// another's name.
#[cfg(all(feature = "automation", target_os = "macos"))]
pub(crate) fn describe(window: &Window) -> String {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_graphics::CGColorSpace;
    use objc2_quartz_core::CAMetalLayer;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return "unknown".into() };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return "unknown".into() };
    // SAFETY: as in `tag_srgb`.
    let view: &AnyObject = unsafe { handle.ns_view.cast::<AnyObject>().as_ref() };
    let layer: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![view, layer] };
    let Some(Ok(layer)) = layer.map(|l| l.downcast::<CAMetalLayer>()) else { return "no metal layer".into() };
    match layer.colorspace() {
        None => "none".into(),
        Some(space) => CGColorSpace::name(Some(&*space)).map_or_else(|| "unnamed".into(), |n| n.to_string()),
    }
}

#[cfg(all(feature = "automation", not(target_os = "macos")))]
pub(crate) fn describe(_window: &Window) -> String {
    "n/a".into()
}
