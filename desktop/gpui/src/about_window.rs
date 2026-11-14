// Threadbear > About Threadbear: a small window of its own in place of the
// stock panel, with the bear waving once as it opens and a few lines from what
// the app already knows (Swift's DenWindow; a den is where a bear lives). The
// lines are threadbear_core::den's. The bear is the Swift app's line bear
// (BearGeometry's wave pose) drawn out once as an SVG, in the top half of a
// square twice as tall, so turning it about the picture's middle turns it
// about its feet.
use crate::app::Threadbear;
use crate::style::{self, px};
use gpui_kit::component::{v_flex, ActiveTheme as _, Icon};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;
use threadbear_core::den;

const BEAR_WAVE: &[u8] = include_bytes!("../icons/bear-wave.svg");
const WIDTH: f32 = 320.;
const BEAR: f32 = 80.;

/// The window while it's open, so About again brings it forward rather than
/// opening another (and doesn't wave again).
struct AboutWindow(Option<AnyWindowHandle>);
impl Global for AboutWindow {}

/// What the window says, as of its opening.
#[derive(Clone)]
pub(crate) struct AboutView {
    lines: Vec<String>,
    build: Option<String>,
    short: Option<String>,
}

impl Threadbear {
    /// About Threadbear (`DenWindow.show`).
    pub(crate) fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = cx.try_global::<AboutWindow>().and_then(|a| a.0) {
            if handle.update(cx, |_, w, _| w.activate_window()).is_ok() {
                return;
            }
        }
        let now = crate::now_ms();
        // The Mac's own time zone, for which day a thread was made on.
        let local = threadbear_core::sidebar::local_now().timezone();
        let launch = crate::installed::launch();
        let short = launch.build.as_ref().map(|b| b.short.clone()).filter(|s| !s.is_empty());
        let view = AboutView {
            lines: den::lines(self.threads.values().map(|t| &**t), now, &local),
            build: launch.build.as_ref().and_then(|b| den::build(&b.short, &b.built_at, &local)),
            short,
        };
        // Its own height from what it says, as SwiftUI fits the Swift one to
        // its content: the bear, the name, a line each, the two at the foot.
        let height = 20. + BEAR + 14. + 20. + if view.lines.is_empty() { 0. } else { 10. + 22. * view.lines.len() as f32 } + 16. + (if view.build.is_some() { 2. } else { 1. }) * 15. + 4. + 32.;
        // Over the top of the main window, a little down, as the Swift one sits.
        let main = window.bounds();
        let origin = point(main.center().x - px(WIDTH / 2.), main.origin.y + px((f32::from(main.size.height) / 8.).min(70.)));
        let bounds = Bounds { origin, size: size(px(WIDTH), px(height)) };
        // A scripted run's window stays behind whoever's using the Mac.
        let scripted = crate::installed::launch().scripted;
        cx.defer(move |cx| {
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions { title: Some("About Threadbear".into()), appears_transparent: true, traffic_light_position: None }),
                    focus: !scripted,
                    is_resizable: false,
                    is_minimizable: false,
                    app_id: Some("threadbear".into()),
                    ..Default::default()
                },
                |window, cx| {
                    window.on_window_should_close(cx, |_, cx| {
                        cx.set_global(AboutWindow(None));
                        true
                    });
                    cx.new(|_| view)
                },
            );
            if let Ok(handle) = opened {
                cx.set_global(AboutWindow(Some(handle.into())));
            }
        });
    }
}

/// The About window while it's open, for a script's `screenshot` of it.
#[cfg(feature = "automation")]
pub(crate) fn handle(cx: &mut App) -> Option<AnyWindowHandle> {
    cx.try_global::<AboutWindow>().and_then(|a| a.0)
}

/// The About window's lines, while it's open, for the specs.
#[cfg(feature = "automation")]
pub(crate) fn dump(cx: &mut App) -> Option<Vec<String>> {
    let handle = cx.try_global::<AboutWindow>().and_then(|a| a.0)?;
    let view = handle.downcast::<AboutView>()?;
    view.read(cx).ok().map(|v| {
        let mut out = vec!["Threadbear".to_owned()];
        out.extend(v.lines.iter().cloned());
        out.extend(v.build.iter().cloned());
        out.push(den::CREDIT.to_owned());
        out
    })
}

/// The commit in the code face, a touch brighter than the words round it.
fn build_line(line: &str, short: Option<&str>, mono: SharedString, commit: Hsla) -> AnyElement {
    let Some((before, after)) = short.and_then(|s| line.split_once(s).map(|(b, a)| (b.to_owned(), (s.to_owned(), a.to_owned())))) else {
        return div().child(line.to_owned()).into_any_element();
    };
    let (short, after) = after;
    div()
        .flex()
        .flex_wrap()
        .justify_center()
        .child(before.replace(' ', "\u{a0}"))
        .child(div().font_family(mono).text_size(px(11.)).text_color(commit).child(short))
        .child(after.replace(' ', "\u{a0}"))
        .into_any_element()
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, secondary) = (cx.theme().foreground, style::secondary_text(cx));
        let reduce = cx.reduce_motion();
        let bear = Icon::default().data(BEAR_WAVE).w(px(BEAR)).h(px(BEAR * 2.)).text_color(fg.opacity(0.85));
        // A tilt from its feet, once, as it opens: -9°, 7°, -5°, then still,
        // in the Swift keyframes' times.
        let bear: AnyElement = if reduce {
            bear.into_any_element()
        } else {
            bear.with_animation("about-wave", Animation::new(Duration::from_millis(900)), |bear, t| bear.rotate(gpui_kit::radians(wave_angle(t).to_radians()))).into_any_element()
        };
        let body = v_flex()
            .w_full()
            .items_center()
            .px(px(26.))
            .pt(px(20.))
            .pb(px(24.))
            // Only the top half of the bear's picture is bear: the rest hangs
            // below its feet, under what follows.
            .child(div().relative().w(px(BEAR)).h(px(BEAR)).child(div().absolute().top_0().left_0().child(bear)))
            .child(div().pt(px(14.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(fg).child("Threadbear"))
            .when(!self.lines.is_empty(), |s| {
                s.child(v_flex().pt(px(10.)).items_center().text_size(px(style::size::LABEL)).text_color(secondary).children(self.lines.iter().map(|l| div().min_h(px(22.)).flex().items_center().text_center().child(l.clone()))))
            })
            .child(
                v_flex()
                    .pt(px(16.))
                    .gap(px(4.))
                    .items_center()
                    .text_center()
                    .text_size(px(11.5))
                    .text_color(secondary.opacity(0.75))
                    .children(self.build.as_deref().map(|b| build_line(b, self.short.as_deref(), cx.theme().mono_font_family.clone(), secondary)))
                    .child(den::CREDIT),
            );
        let _ = window;
        let root = div().id("about").size_full().bg(cx.theme().background).child(body);
        // Comes up from a touch small and clear, as the Swift one does.
        if reduce {
            root.into_any_element()
        } else {
            root.with_animation("about-open", Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()), |el, t| el.opacity(t)).into_any_element()
        }
    }
}

/// The wave's angle in degrees, `t` from 0 to 1 over the 0.9 s it takes:
/// eased from each keyframe to the next, as SwiftUI's cubic keyframes.
fn wave_angle(t: f32) -> f32 {
    const KEYS: [(f32, f32); 5] = [(0., 0.), (0.225, -9.), (0.45, 7.), (0.666, -5.), (0.9, 0.)];
    let at = t.clamp(0., 1.) * 0.9;
    for pair in KEYS.windows(2) {
        let ((t0, a0), (t1, a1)) = (pair[0], pair[1]);
        if at <= t1 {
            let u = ((at - t0) / (t1 - t0)).clamp(0., 1.);
            let eased = u * u * (3. - 2. * u);
            return a0 + (a1 - a0) * eased;
        }
    }
    0.
}

#[cfg(test)]
mod tests {
    use super::wave_angle;

    #[test]
    fn the_wave_goes_through_its_keyframes_and_ends_still() {
        assert_eq!(wave_angle(0.), 0.);
        assert!((wave_angle(0.25) + 9.).abs() < 0.01);
        assert!((wave_angle(0.5) - 7.).abs() < 0.01);
        assert_eq!(wave_angle(1.), 0.);
    }
}
