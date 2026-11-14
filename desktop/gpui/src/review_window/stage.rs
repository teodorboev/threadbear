// The stage (ReviewWindow.swift's `stage`): which option and shot this is
// over it, the shot itself (or all of them side by side), and under it the
// arrows, the option's other shots and a mark for every shot of every option.
//
// The shot zoomed in is threadbear_core::review::StageZoom; this draws the
// image where it says, at the stage's own points (a native scroll view's, so
// not the app's text zoom), and hands it the pointer.
use super::ReviewWindow;
use crate::style::{self, px};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use threadbear_core::open_policy::Origin;
use threadbear_core::reducer::{ImageRef, ReviewOption, ReviewShot};
use threadbear_core::review::{self as rules, GalleryLayout, Point as StagePoint, ReviewMode, ScrollPhase, StageZoom, ZoomStep};

/// ReviewWindow's spacing, in the stage's points.
const STAGE_INSET: f32 = 14.;
const CELL_SPACING: f32 = 12.;
const CELL_PADDING: f32 = 8.;
const CAPTION_HEIGHT: f32 = 20.;
/// The film strip's pictures and the rail's, cropped from the top left.
const STRIP_THUMB: (f32, f32) = (92., 58.);
/// What a wheel's line is in points when Shift turns it sideways.
const WHEEL_LINE: f32 = 16.;

/// What the stage holds between frames.
#[derive(Default)]
pub(crate) struct StageState {
    /// The images' room as last drawn, in window points: the zoom's view and
    /// the gallery's room come from it, a frame behind a resize.
    room: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The shot on show, zoomed and moved, with the image it's for and
    /// whether that was today's version flicked to.
    zoom: Option<Zoomed>,
    /// A zoom asked for before the stage had its size.
    pending: Option<ZoomStep>,
    /// The minimap's being dragged.
    minimap_drag: bool,
    marks_scroll: ScrollHandle,
    strip_scroll: ScrollHandle,

    /// The option and shot the row under the stage last scrolled to.
    followed: Option<(usize, usize)>,
    /// A page shot on show, sealed with its scripts off (`review.media`):
    /// its file and its view, dropped once another shot shows.
    pub(crate) page: Option<(String, Rc<threadbear_web::PanelPage>)>,
    /// Why the page shot on show couldn't show in place, by its file.
    page_failed: Option<(String, String)>,
    /// What the page is waiting for (the runtime), for the dump.
    page_waiting: Option<String>,
    /// The web runtime watched, so the stage draws again as it starts.
    runtime_watch: Option<Subscription>,
}

struct Zoomed {
    path: String,
    peeking: bool,
    zoom: StageZoom,
}

impl StageState {
    /// Why the last page shot couldn't show in place, for the dump.
    pub(crate) fn page_failed_reason(&self) -> Option<String> {
        self.page_failed.as_ref().map(|(_, why)| why.clone()).or_else(|| self.page_waiting.clone())
    }

    pub(crate) fn zoom_requested(&mut self, step: ZoomStep) {
        match &mut self.zoom {
            Some(z) => z.zoom.apply(step),
            None => self.pending = Some(step),
        }
    }

    pub(crate) fn is_zoomed(&self) -> bool {
        self.zoom.as_ref().is_some_and(|z| z.zoom.is_zoomed())
    }

    pub(crate) fn minimap_showing(&self) -> bool {
        self.zoom.as_ref().is_some_and(|z| z.zoom.minimap().is_some())
    }

    fn room(&self) -> Option<Bounds<Pixels>> {
        self.room.get()
    }

    /// The zoom for the shot about to be drawn: kept for the same image; for
    /// today's version flicked to (or back), the same part at the same zoom,
    /// or there'd be nothing to compare; for any other, fitted afresh.
    fn follow(&mut self, path: &str, peeking: bool, doc: (f64, f64), view: (f64, f64)) -> &mut StageZoom {
        let keep = self.zoom.as_ref().is_some_and(|z| z.path == path);
        if !keep {
            let swapped = self.zoom.as_ref().is_some_and(|z| z.peeking != peeking);
            match (&mut self.zoom, swapped) {
                (Some(z), true) => {
                    z.zoom.set_view(view.0, view.1);
                    z.zoom.swap_image(doc.0, doc.1);
                    z.path = path.to_owned();
                    z.peeking = peeking;
                }
                _ => {
                    let mut zoom = StageZoom::new(doc.0, doc.1, view.0, view.1);
                    if let Some(step) = self.pending.take() {
                        zoom.apply(step);
                    }
                    self.zoom = Some(Zoomed { path: path.to_owned(), peeking, zoom });
                }
            }
        }
        let z = self.zoom.as_mut().expect("just made");
        z.peeking = peeking;
        if let Some(step) = self.pending.take() {
            z.zoom.apply(step);
        }
        z.zoom.set_view(view.0, view.1);
        &mut z.zoom
    }
}

/// An image's size in pixels, or the Swift app's stand-in when the runner
/// didn't say (`sourceSize`).
fn pixels(image: &ImageRef) -> (f64, f64) {
    match (image.width, image.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w as f64, h as f64),
        _ => (1100., 760.),
    }
}

fn pt(v: f64) -> Pixels {
    gpui_kit::px(v as f32)
}

impl ReviewWindow {
    pub(super) fn render_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let single = self.session.mode == ReviewMode::Single;
        let caption = if single { self.session.shown_option().cloned().map(|o| self.render_caption(&o, cx)) } else { None };
        let images = if single { self.render_shot(window, cx) } else { self.render_gallery(window, cx) };
        let stepper = self.render_stepper(cx);
        let room = self.stage.room.clone();
        v_flex()
            .id("review-stage")
            // Its own width whatever the image's: the rail keeps its 340.
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(hsla(0., 0., 0., if dark { 0.22 } else { 0.05 }))
            .children(caption)
            .child(
                div()
                    .id("review-images")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(canvas(move |bounds, _, _| room.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full())
                    .child(images),
            )
            .child(stepper)
            .into_any_element()
    }

    /// Which option and shot this is, and the ways to try it: today's version
    /// flicked to, the option running live.
    fn render_caption(&mut self, option: &ReviewOption, cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        let shown = self.session.shown_shot().unwrap_or_else(|| option.shot(0));
        let place = rules::caption_where(option, &shown);
        let mut buttons: Vec<AnyElement> = vec![];
        if let (Some(baseline), true) = (self.session.baseline().cloned(), self.session.can_peek()) {
            let peeking = self.session.peeking;
            let focused = self.session.focused_option().map(|o| o.id.clone()).unwrap_or_else(|| "it".into());
            let tip = if peeking { format!("Back to {focused} (or let go of Space)") } else { format!("Flick to {}: click, or hold Space", baseline.title) };
            let (fg, bg) = if peeking { (gpui_kit::white(), style::control_accent(cx)) } else { (style::label_text(cx), style::primary_opacity(cx, 0.08)) };
            buttons.push(
                // Its words in a 140 wide frame, as the Swift label's
                // `frame(maxWidth: 140)` takes all of that whatever they say.
                crate::highlights::tipped(capsule_button("review-peek", Icon::new(IconName::ArrowLeftRight), 16., 12., &baseline.title, fg, bg).w(px(140. + 22.)).justify_center(), tip)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_peeking(!peeking, cx)))
                    .into_any_element(),
            );
        }
        if let Some(url) = option.url.clone() {
            let tip = format!("Try {} running live: {url}", option.id);
            buttons.push(
                crate::highlights::tipped(capsule_button("review-open-live", Icon::new(IconName::ArrowUpRight), 12., 7., "Open live", style::label_text(cx), style::primary_opacity(cx, 0.08)), tip)
                    // Only a web page, which is all an option's url is for: a
                    // runner (maybe another Mac's) sends it, and anything else
                    // would open whatever it named here.
                    .on_click(move |_, _, cx| {
                        crate::open_gate::open_web(&url, cx);
                    })
                    .into_any_element(),
            );
        }
        let height = if buttons.is_empty() { style::natural_line(style::size::WORK + 2.) } else { 26. };
        h_flex()
            .flex_none()
            .w_full()
            .px(px(22.))
            .pt(px(14.))
            .gap(px(10.))
            .items_center()
            .h(px(14. + height))
            // On the buttons' first baseline, as the Swift row lines them up:
            // 2 higher than the middle.
            .child(div().flex_none().mb(px(4.)).max_w(px(520.)).truncate().text_size(px(style::size::WORK + 2.)).line_height(px(style::natural_line(style::size::WORK + 2.))).font_weight(FontWeight::SEMIBOLD).child(option.label()))
            .child(div().mb(px(4.)).min_w_0().truncate().text_size(px(style::size::WORK)).line_height(px(style::natural_line(style::size::WORK))).text_color(secondary).child(place))
            .child(div().flex_1().min_w(px(8.)))
            .children(buttons)
            .into_any_element()
    }

    /// One shot with the whole stage to zoom in. A click goes back to all of
    /// them; zoomed in, a press is for dragging.
    fn render_shot(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (Some(shot), Some(room)) = (self.session.shown_shot(), self.stage.room()) else { return div().into_any_element() };
        if shot.media.is_some() {
            return self.render_media(&shot, room, window, cx);
        }
        let scale = f64::from(window.scale_factor()).max(1.);
        let (w, h) = pixels(&shot.image);
        let peeking = self.session.peeking && self.session.can_peek();
        let path = shot.image.path.clone();
        let zoom = self.stage.follow(&path, peeking, (w / scale, h / scale), (f64::from(room.size.width), f64::from(room.size.height)));
        let image = zoom.image_rect();
        let minimap = zoom.minimap();
        let fitted = zoom.is_fitted();
        let dragging = zoom.dragging();
        self.after_zoom();
        let id = self.session.focused_option().map(|o| o.id.clone()).unwrap_or_default();
        let unavailable = style::secondary_text(cx);
        let mut stage = div()
            .id("review-shot")
            .absolute()
            .size_full()
            .overflow_hidden()
            .cursor(if fitted { CursorStyle::Arrow } else if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.stage_pressed(ev, window, cx)))
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _, cx| this.stage_scrolled(ev, cx)))
            .on_pinch(cx.listener(|this, ev: &PinchEvent, _, cx| this.stage_pinched(ev, cx)))
            .child(
                img(crate::rows::markdown::file_picture(&path))
                    .absolute()
                    .left(pt(image.x))
                    .top(pt(image.y))
                    .w(pt(image.width))
                    .h(pt(image.height))
                    .object_fit(ObjectFit::Fill)
                    .with_fallback(move || {
                        div().size_full().flex().items_center().justify_center().child(Icon::new(IconName::ImageOff).with_size(gpui_kit::px(40.)).text_color(unavailable)).into_any_element()
                    }),
            );
        if let Some(map) = minimap {
            stage = stage.child(self.render_minimap(&path, map, cx));
        }
        // Zoomed in, no tip over the image: it's for dragging, and a tip
        // would sit over what's being looked at.
        if fitted {
            crate::highlights::tipped(stage, format!("{id}: click to see them all, scroll to zoom")).into_any_element()
        } else {
            stage.into_any_element()
        }
    }

    /// A clip or a page. On a Mac the clip plays on the stage in the
    /// system's player, round and round with its controls and its sound on,
    /// as the Swift window's does: some clips are there to be heard. A page,
    /// or a clip anywhere else (or one the open gate won't play), is its
    /// still, fitted, with a button that opens it where it can, through the
    /// gate (the clip in the app this computer plays videos in, the page in
    /// the browser). The Swift window shows the page in place with its
    /// scripts off.
    fn render_media(&mut self, shot: &ReviewShot, room: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Zoom is for pictures: a clip or page has its own ways to move round.
        self.stage.zoom = None;
        self.after_zoom();
        let fit = GalleryLayout::fit(rules::aspect(&shot.image), f64::from(room.size.width), f64::from(room.size.height));
        // A page in place, sealed as the pane's are: only its own named
        // assets, no network, its scripts off (the Swift window's too).
        if let Some(page) = shot.page() {
            if let Some(view) = self.stage_page(page, window, cx) {
                return div().id("review-media").absolute().size_full().flex().items_center().justify_center().child(div().flex_none().w(pt(fit.0)).h(pt(fit.1)).child(view)).into_any_element();
            }
        }
        if let Some(video) = shot.video().filter(|_| crate::media_view::SHOWN_HERE) {
            let path = video.to_owned();
            // Fitted as its still is, so the controls sit on the film.
            if let Ok(view) = crate::media_view::slot(&stage_key(video), window, |opening| crate::open_gate::show_media(&path, Origin::ThisMac, crate::media_view::Kind::StageVideo, opening)) {
                return div().id("review-media").absolute().size_full().flex().items_center().justify_center().child(div().flex_none().w(pt(fit.0)).h(pt(fit.1)).child(view)).into_any_element();
            }
        }
        let play_tip = if crate::media_view::SHOWN_HERE { "Play it in the app this computer plays videos in" } else { "Play it in the app this computer plays videos in: this window can't play clips yet" };
        let (icon, words, tip, path) = match (shot.video(), shot.page()) {
            (Some(video), _) => (IconName::Play, "Play the clip", play_tip, video.to_owned()),
            (_, Some(page)) => (IconName::Globe, "Open the page", "Open it in your browser: this window can't show pages yet", page.to_owned()),
            _ => return div().into_any_element(),
        };
        let unavailable = style::secondary_text(cx);
        let button = crate::highlights::tipped(
            h_flex()
                .id("review-open-media")
                .h(px(40.))
                .px(px(18.))
                .gap(px(9.))
                .items_center()
                .rounded_full()
                .bg(hsla(0., 0., 0., 0.72))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.24))
                .shadow_lg()
                .cursor_pointer()
                .hover(|s| s.bg(hsla(0., 0., 0., 0.85)))
                .text_color(gpui_kit::white())
                .child(Icon::new(icon).with_size(px(16.)).text_color(gpui_kit::white()))
                .child(div().text_size(px(style::size::BODY)).font_weight(FontWeight::MEDIUM).child(words)),
            tip,
        )
        .on_click(move |_, _, cx| crate::open_gate::open_file(&path, Origin::ThisMac, cx));
        div()
            .id("review-media")
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(pt(fit.0))
                    .h(pt(fit.1))
                    .child(img(crate::rows::markdown::file_picture(&shot.image.path)).size_full().object_fit(ObjectFit::Fill).with_fallback(move || {
                        div().size_full().flex().items_center().justify_center().child(Icon::new(IconName::ImageOff).with_size(gpui_kit::px(40.)).text_color(unavailable)).into_any_element()
                    }))
                    // Dimmed, so it reads as a still with something to open,
                    // and the button stands out over any mockup.
                    .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.35)).flex().items_center().justify_center().child(button)),
            )
            .into_any_element()
    }

    /// The page's view, placed where it's drawn each frame, or None while the
    /// web runtime isn't running here or the gate won't show the file: then
    /// the still and Open the page, as before.
    fn stage_page(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(runtime) = crate::web_runtime::runtime(cx) else {
            self.stage.page_waiting = Some("no web runtime here".into());
            return None;
        };
        // Drawn again as it starts, so the page comes once it's running.
        if self.stage.runtime_watch.is_none() {
            self.stage.runtime_watch = Some(cx.observe(&runtime, |_, _, cx| cx.notify()));
        }
        if runtime.read(cx).state != threadbear_core::web_runtime::State::Ready || !runtime.update(cx, |r, cx| r.ensure_engine(cx)) {
            self.stage.page_waiting = Some(format!("runtime {:?}", runtime.read(cx).state));
            return None;
        }
        self.stage.page_waiting = None;
        if self.stage.page_failed.as_ref().is_some_and(|(p, _)| p == path) {
            return None;
        }
        if self.stage.page.as_ref().is_none_or(|(p, _)| p != path) {
            self.stage.page = None;
            let made = (|| {
                let real = threadbear_core::local_preview::decide_page(path, Origin::ThisMac)?;
                let site = threadbear_core::local_preview::Site::new(&real, None).ok_or_else(|| "its folder couldn't be read".to_owned())?;
                let preview = threadbear_web::PreviewSite::new(std::sync::Arc::new(site), false);
                let parent = crate::web_panels::native_view(window).ok_or_else(|| "the window has no view".to_owned())?;
                let (page, _events) = threadbear_web::PanelPage::open(parent, &preview.page_url(), threadbear_web::PanelKind::Preview(preview))?;
                Ok::<_, String>(page)
            })();
            match made {
                Ok(page) => self.stage.page = Some((path.to_owned(), Rc::new(page))),
                Err(why) => {
                    self.stage.page_failed = Some((path.to_owned(), why));
                    return None;
                }
            }
        }
        let page = self.stage.page.as_ref()?.1.clone();
        // The title bar's menu draws under a native view: out of sight while it's up.
        let covered = self.more_open;
        Some(
            canvas(
                move |bounds, window, _| {
                    let height = f32::from(window.viewport_size().height);
                    page.set_bounds(f32::from(bounds.origin.x), f32::from(bounds.origin.y), f32::from(bounds.size.width), f32::from(bounds.size.height), height);
                    page.set_hidden(covered);
                },
                |_, _, _, _| {},
            )
            .size_full()
            .into_any_element(),
        )
    }

    /// The whole image small, top right, with a box for the part on show, while
    /// some of it is off the stage; drag or click it to go there.
    fn render_minimap(&mut self, path: &str, map: rules::Minimap, cx: &mut Context<Self>) -> AnyElement {
        let f = map.frame;
        let s = map.shown;
        crate::highlights::tipped(
            div()
                .id("review-minimap")
                .absolute()
                .left(pt(f.x))
                .top(pt(f.y))
                .w(pt(f.width))
                .h(pt(f.height))
                .rounded(gpui_kit::px(4.))
                .overflow_hidden()
                .bg(hsla(0., 0., 0., 0.55))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.25))
                .cursor(CursorStyle::Arrow)
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.stage.minimap_drag = true;
                    this.minimap_moved(ev.position, cx);
                }))
                .child(img(crate::rows::markdown::file_picture(path)).absolute().size_full().object_fit(ObjectFit::Fill).opacity(0.8))
                .child(div().absolute().left(pt(s.x)).top(pt(s.y)).w(pt(s.width)).h(pt(s.height)).border(gpui_kit::px(1.5)).border_color(gpui_kit::white()).bg(hsla(0., 0., 1., 0.2))),
            "Drag to move around the image",
        )
        .into_any_element()
    }

    /// All of them side by side, each its first shot, as many columns as make
    /// them biggest (`ReviewGalleryLayout`).
    fn render_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(room) = self.stage.room() else { return div().into_any_element() };
        let options = self.session.review().options.clone();
        let (w, h) = ((f64::from(room.size.width) - f64::from(STAGE_INSET) * 2.).max(0.), (f64::from(room.size.height) - f64::from(STAGE_INSET) * 2.).max(0.));
        let aspects: Vec<f64> = options.iter().map(|o| rules::aspect(&o.image)).collect();
        let chrome_h = f64::from(CELL_PADDING) * 2. + f64::from(CAPTION_HEIGHT);
        let layout = GalleryLayout::best(&aspects, w, h, f64::from(CELL_SPACING), f64::from(CELL_PADDING) * 2., chrome_h);
        let mut rows = v_flex().gap(gpui_kit::px(CELL_SPACING)).items_center();
        for first in (0..options.len()).step_by(layout.columns.max(1)) {
            let mut row = h_flex().gap(gpui_kit::px(CELL_SPACING));
            for index in first..(first + layout.columns).min(options.len()) {
                row = row.child(self.render_cell(index, &options[index], layout.cell_width, layout.cell_height, window, cx));
            }
            rows = rows.child(row);
        }
        div().absolute().size_full().flex().items_center().justify_center().child(rows).into_any_element()
    }

    /// An option among them all: its first shot, how many it has, and the
    /// agent's pick marked; a tick's outline round it once it's picked.
    fn render_cell(&mut self, index: usize, option: &ReviewOption, width: f64, height: f64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let picked = self.session.draft().picked_ids.contains(&option.id);
        let count = option.shot_count();
        let recommended = self.session.review().recommended.as_deref() == Some(option.id.as_str());
        let room = ((width - f64::from(CELL_PADDING) * 2.).max(0.), (height - f64::from(CELL_PADDING) * 2. - f64::from(CAPTION_HEIGHT)).max(0.));
        let fit = GalleryLayout::fit(rules::aspect(&option.image), room.0, room.1);
        let accent = style::control_accent(cx);
        let id_color = if picked { accent } else { style::secondary_text(cx) };
        let path = option.image.path.clone();
        let unavailable = style::secondary_text(cx);
        let caption = h_flex()
            .flex_none()
            .h(gpui_kit::px(CAPTION_HEIGHT))
            .gap(px(6.))
            .items_start()
            .child(div().text_size(px(style::size::DETAIL)).line_height(px(style::natural_line(style::size::DETAIL))).font_weight(FontWeight::SEMIBOLD).text_color(id_color).child(option.id.clone()))
            .when(count > 1, |s| s.child(div().text_size(px(style::size::DETAIL)).line_height(px(style::natural_line(style::size::DETAIL))).text_color(style::tertiary_label(cx)).child(format!("{count} shots"))))
            .when(recommended, |s| s.child(super::rail::recommended_mark(cx)));
        let picture = crate::highlights::tipped(div().id(("review-cell", index)).relative().flex_none().w(pt(fit.0)).h(pt(fit.1)).cursor_pointer(), format!("{}: click to enlarge", option.id))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.take_keys(window, cx);
                this.enlarge(index, 0, cx);
            }))
            .child(img(crate::rows::markdown::file_picture(&path)).size_full().object_fit(ObjectFit::Fill).with_fallback(move || {
                div().size_full().flex().items_center().justify_center().child(Icon::new(IconName::ImageOff).with_size(gpui_kit::px(28.)).text_color(unavailable)).into_any_element()
            }))
            // Its first shot a clip or a page: a still, marked as one, and on
            // a Mac a clip plays over it, muted and with no controls, as they
            // all play at once to be compared moving. A click on it goes
            // through the player to this cell, which enlarges it.
            .children(media_badge(&option.shot(0)))
            .children(self.cell_clip(index, &option.shot(0), window));
        v_flex()
            .relative()
            .flex_none()
            .w(pt(width))
            .h(pt(height))
            .p(gpui_kit::px(CELL_PADDING))
            .child(caption)
            .child(div().flex_1().min_h_0().flex().items_center().justify_center().child(picture))
            .when(picked, |s| s.child(div().absolute().inset_0().rounded(gpui_kit::px(10.)).border_2().border_color(accent)))
            .into_any_element()
    }

    /// The row under the stage: back and next at its ends, and between them the
    /// option's other shots from the left and the marks from the right.
    fn render_stepper(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let single = self.session.mode == ReviewMode::Single;
        let back_tip = if single { "Previous (\u{2190})" } else { "Back to the last one (\u{2190})" };
        let next_tip = if self.session.next_is_all() { "See them all and choose (\u{2192})" } else { "Next (\u{2192})" };
        let back = step_button("review-previous", IconName::ChevronLeft, back_tip, self.session.can_step_back(), false, cx).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.browse(-1, cx)));
        let next = step_button("review-next", IconName::ChevronRight, next_tip, self.session.can_step_forward(), true, cx).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.browse(1, cx)));
        let strip_option = self.session.focused_option().filter(|o| single && o.shot_count() > 1).cloned();
        // What each wants, and what they get (`ReviewStepperLayout.share`).
        let width = self.stage.room().map_or(0., |r| f32::from(r.size.width));
        let marks_wanted = self.marks_width();
        let strip_wanted = strip_option.as_ref().map_or(0., |o| {
            let n = o.shot_count() as f32;
            n * STRIP_THUMB.0 + (n - 1.) * 8. + 6.
        });
        let room = width - 44. - 100. - 18. * if strip_option.is_some() { 3. } else { 2. };
        let (strip_width, marks_width) = rules::stepper_share(f64::from(room), f64::from(strip_wanted), f64::from(marks_wanted));
        self.follow_marks();
        let mut row = h_flex().flex_none().w_full().px(px(22.)).py(px(12.)).gap(px(18.)).items_center().child(back);
        if let Some(option) = strip_option {
            row = row.child(self.render_filmstrip(&option, strip_width as f32, cx));
        }
        row.child(div().flex_1())
            .child(self.render_marks(marks_width as f32, cx))
            .child(next)
            .into_any_element()
    }

    /// How wide the marks and the grid button want to be: each option's letter
    /// and a 13 point slot per shot, 12 between options, then the button.
    fn marks_width(&self) -> f32 {
        // As drawn last time: what the row shows plus what it scrolls past.
        // Until then the letters' widths are guessed.
        let drawn = self.stage.marks_scroll.bounds().size.width + self.stage.marks_scroll.max_offset().x;
        let marks = if drawn > gpui_kit::px(0.) {
            f32::from(drawn)
        } else {
            let options = &self.session.review().options;
            let letters: f32 = options.iter().map(|o| o.id.chars().count() as f32 * 8. + 2. + o.shot_count() as f32 * 13.).sum();
            letters + 12. * options.len().saturating_sub(1) as f32
        };
        marks + 12. + 26.
    }

    /// Keeps the dot on show in view, not its option: an option with dozens of
    /// shots is wider than the row.
    fn follow_marks(&mut self) {
        let here = (self.session.focused(), self.session.shot());
        if self.session.mode == ReviewMode::Single && self.stage.followed != Some(here) {
            self.stage.followed = Some(here);
            // The row's children: each option's letter, then its dots.
            let before: usize = self.session.review().options.iter().take(here.0).map(|o| 1 + o.shot_count()).sum();
            self.stage.marks_scroll.scroll_to_item(before + 1 + here.1);
            self.stage.strip_scroll.scroll_to_item(here.1);
        }
    }

    /// Every option's letter and a dot per shot, scrolling sideways when
    /// there are too many, then the grid button. A reference's dots are
    /// hollow: it's there to look at, not to pick.
    fn render_marks(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let single = self.session.mode == ReviewMode::Single;
        let (accent, primary, secondary) = (style::control_accent(cx), style::label_text(cx), style::secondary_text(cx));
        let faint = style::secondary_text(cx).opacity(0.45);
        let mut dots = h_flex().id("review-marks").flex_none().max_w(px((width - 12. - 26.).max(0.))).items_center().overflow_x_scroll().track_scroll(&self.stage.marks_scroll);
        let options = self.session.review().options.clone();
        for (index, option) in options.iter().enumerate() {
            let picked = self.session.draft().picked_ids.contains(&option.id);
            let here = single && self.session.focused() == index;
            let letter_color = if picked { accent } else if here { primary } else { secondary };
            dots = dots.child(div().flex_none().when(index > 0, |s| s.ml(px(12.))).pr(px(2.)).text_size(px(style::size::DETAIL - 1.)).font_weight(FontWeight::SEMIBOLD).text_color(letter_color).child(option.id.clone()));
            for shot in 0..option.shot_count() {
                let current = here && self.session.shot() == shot;
                let color = if picked { accent } else if current || here { primary } else { faint };
                let side = if current { 10. } else { 7. };
                let dot = div().size(px(side)).rounded_full().map(|d| if option.reference { d.border(px(1.5)).border_color(color) } else { d.bg(color) });
                dots = dots.child(
                    crate::highlights::tipped(div().id(SharedString::from(format!("review-mark-{}-{shot}", option.id))).flex_none().w(px(13.)).h(px(26.)).flex().items_center().justify_center().cursor_pointer(), rules::shot_tip(option, shot))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.enlarge(index, shot, cx)))
                        .child(dot),
                );
            }
        }
        let all = !single;
        let grid = crate::highlights::tipped(div().id("review-grid").flex_none().size(px(26.)).flex().items_center().justify_center().cursor_pointer(), "See them all and choose (Esc)")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.show_all(cx)))
            .child(Icon::new(IconName::LayoutGrid).with_size(px(13.)).text_color(if all { primary } else { secondary }));
        h_flex().flex_none().gap(px(12.)).items_center().child(dots).child(grid).into_any_element()
    }

    /// The option's shots, scrolling sideways once there are more than the
    /// row has room for; the one on show outlined.
    fn render_filmstrip(&mut self, option: &ReviewOption, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let (primary, secondary) = (style::label_text(cx), style::secondary_text(cx));
        let focused = self.session.focused();
        let mut strip = h_flex().id("review-filmstrip").p(px(3.)).gap(px(8.)).overflow_x_scroll().track_scroll(&self.stage.strip_scroll);
        for (index, shot) in option.all_shots().iter().enumerate() {
            let current = index == self.session.shot();
            let title = shot.title.clone().unwrap_or_else(|| format!("{}", index + 1));
            strip = strip.child(
                crate::highlights::tipped(v_flex().id(("review-strip", index)).flex_none().gap(px(4.)).cursor_pointer(), rules::shot_tip(option, index))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.enlarge(focused, index, cx)))
                    .child(
                        div()
                            .relative()
                            .child(thumb(&shot.image, STRIP_THUMB.0, STRIP_THUMB.1, 4., cx).opacity(if current { 1. } else { 0.55 }))
                            .when(current, |s| s.child(div().absolute().top(px(-3.)).left(px(-3.)).w(px(STRIP_THUMB.0 + 6.)).h(px(STRIP_THUMB.1 + 6.)).rounded(px(5.)).border_2().border_color(primary))),
                    )
                    .child(div().w(px(STRIP_THUMB.0)).truncate().text_size(px(style::size::DETAIL - 1.)).line_height(px(style::natural_line(style::size::DETAIL - 1.))).text_color(if current { primary } else { secondary }).child(title)),
            );
        }
        div().flex_none().w(px(width.max(0.))).overflow_hidden().child(strip).into_any_element()
    }

    // The pointer on the stage.

    fn stage_point(&self, at: Point<Pixels>) -> Option<StagePoint> {
        let room = self.stage.room()?;
        Some(StagePoint { x: f64::from(at.x - room.origin.x), y: f64::from(at.y - room.origin.y) })
    }

    fn stage_pressed(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.take_keys(window, cx);
        let Some(at) = self.stage_point(ev.position) else { return };
        let Some(z) = self.stage.zoom.as_mut() else { return };
        // Fitted, a press is a click: back to all of them.
        if z.zoom.press(at, ev.click_count) {
            self.show_all(cx);
            return;
        }
        self.after_zoom();
        cx.notify();
    }

    /// A drag from the stage or the minimap, wherever the pointer's gone in
    /// the window.
    pub(super) fn stage_dragged(&mut self, ev: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if ev.pressed_button != Some(MouseButton::Left) {
            if self.stage.minimap_drag || self.stage.zoom.as_ref().is_some_and(|z| z.zoom.dragging()) {
                self.stage_released(cx);
            }
            return;
        }
        if self.stage.minimap_drag {
            self.minimap_moved(ev.position, cx);
            return;
        }
        let Some(at) = self.stage_point(ev.position) else { return };
        if let Some(z) = self.stage.zoom.as_mut().filter(|z| z.zoom.dragging()) {
            z.zoom.drag_to(at);
            cx.notify();
        }
    }

    pub(super) fn stage_released(&mut self, cx: &mut Context<Self>) {
        self.stage.minimap_drag = false;
        if let Some(z) = self.stage.zoom.as_mut().filter(|z| z.zoom.dragging()) {
            z.zoom.release();
            cx.notify();
        }
    }

    fn minimap_moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(at) = self.stage_point(at) else { return };
        if let Some(z) = self.stage.zoom.as_mut() {
            if let Some(map) = z.zoom.minimap() {
                z.zoom.minimap_to(StagePoint { x: at.x - map.frame.x, y: at.y - map.frame.y });
                cx.notify();
            }
        }
    }

    /// A wheel zooms (Shift turns it sideways); a trackpad swipe zooms from
    /// fit and moves round once zoomed in, decided as it starts.
    fn stage_scrolled(&mut self, ev: &ScrollEvent, cx: &mut Context<Self>) {
        let at = self.stage_point(ev.position);
        let Some(z) = self.stage.zoom.as_mut() else { return };
        let held = ev.modifiers.platform || ev.modifiers.control || ev.modifiers.alt;
        match ev.delta {
            ScrollDelta::Lines(lines) => {
                if !z.zoom.wheel(f64::from(lines.y), ev.modifiers.shift, at) {
                    z.zoom.scroll_by(f64::from(lines.x * WHEEL_LINE), f64::from(lines.y * WHEEL_LINE));
                }
            }
            ScrollDelta::Pixels(delta) => {
                let phase = match ev.touch_phase {
                    TouchPhase::Started => ScrollPhase::Started,
                    TouchPhase::Ended | TouchPhase::Cancelled => ScrollPhase::Ended,
                    TouchPhase::Moved => ScrollPhase::Moved,
                };
                z.zoom.swipe(f64::from(delta.x), f64::from(delta.y), phase, held, at);
            }
        }
        self.after_zoom();
        cx.stop_propagation();
        cx.notify();
    }

    fn stage_pinched(&mut self, ev: &PinchEvent, cx: &mut Context<Self>) {
        let at = self.stage_point(ev.position);
        if let Some(z) = self.stage.zoom.as_mut() {
            z.zoom.pinch(f64::from(ev.delta), at);
            self.after_zoom();
            cx.notify();
        }
    }
}

/// A shot small, cropped from its top left, where a mockup's title and
/// controls are, rather than squashed (`ReviewThumb`).
pub(super) fn thumb(image: &ImageRef, width: f32, height: f32, radius: f32, cx: &App) -> Div {
    let (w, h) = pixels(image);
    let scale = (f64::from(width) / w).max(f64::from(height) / h);
    div()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(style::primary_opacity(cx, 0.08))
        .child(img(crate::rows::markdown::file_picture(&image.path)).w(px((w * scale) as f32)).h(px((h * scale) as f32)).object_fit(ObjectFit::Fill))
}

/// The stage's player for a clip (media_view.rs), one per window.
pub(super) fn stage_key(path: &str) -> String {
    format!("review stage {path}")
}

impl ReviewWindow {
    /// A cell's clip playing over its still, on a Mac: muted, looping, no
    /// controls. One the gate won't play leaves the still.
    fn cell_clip(&mut self, index: usize, shot: &ReviewShot, window: &mut Window) -> Option<AnyElement> {
        let video = shot.video().filter(|_| crate::media_view::SHOWN_HERE)?.to_owned();
        let key = format!("review cell {index} {video}");
        let view = crate::media_view::slot(&key, window, |opening| crate::open_gate::show_media(&video, Origin::ThisMac, crate::media_view::Kind::CellVideo, opening)).ok()?;
        Some(div().absolute().inset_0().child(view).into_any_element())
    }
}

/// A clip's or a page's mark on its still in the grid.
fn media_badge(shot: &ReviewShot) -> Option<AnyElement> {
    let icon = if shot.video().is_some() {
        IconName::Play
    } else if shot.page().is_some() {
        IconName::Globe
    } else {
        return None;
    };
    Some(
        div()
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .size(px(26.))
            .rounded_full()
            .bg(hsla(0., 0., 0., 0.62))
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(icon).with_size(px(13.)).text_color(gpui_kit::white()))
            .into_any_element(),
    )
}

/// The back and next buttons: a chevron in a 50 by 32 capsule, next a shade
/// stronger, as it's the way on.
fn step_button(id: &'static str, icon: IconName, tip: &'static str, enabled: bool, prominent: bool, cx: &App) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let fill = style::primary_opacity(cx, if prominent && enabled { 0.1 } else { 0.05 });
    let fg = if enabled { style::label_text(cx) } else { style::secondary_text(cx).opacity(0.4) };
    crate::highlights::tipped(div().id(id).flex_none().w(px(50.)).h(px(32.)).rounded_full().bg(fill).flex().items_center().justify_center().when(enabled, |s| s.cursor_pointer()), tip)
        .child(Icon::new(icon).with_size(px(15.)).text_color(fg))
}

/// A capsule with a symbol and a few words over the stage (the caption's
/// buttons). The symbol's size and gap are each Swift button's own: its
/// swap symbol is wider than its arrow.
fn capsule_button(id: &'static str, icon: Icon, icon_size: f32, gap: f32, title: &str, fg: Hsla, bg: Hsla) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h(px(26.))
        .px(px(11.))
        .gap(px(gap))
        .items_center()
        .rounded_full()
        .bg(bg)
        .cursor_pointer()
        .text_color(fg)
        .child(icon.with_size(px(icon_size)).text_color(fg))
        .child(div().min_w_0().truncate().text_size(px(style::size::DETAIL)).font_weight(FontWeight::MEDIUM).child(title.to_owned()))
}
