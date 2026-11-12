// The pages the activity bar opens in the main panel beside the sidebar's view
// (Scheduled's, the Librarian's), a port of the Swift app's HomePage: a picture
// of the bear at the job, a few lines on what the view is for, its numbers in
// tiles and anything else under them. Also the parts the views share: the
// view's name at the top of the sidebar (`ModeHeader`), the ⓘ that says more
// (`InfoTip`), and the page's title and status line.
use crate::app::Threadbear;
use crate::sidebar::{icon, Palette, LEADING, OUTER_INSET, TRAILING};
use crate::style;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use threadbear_core::home::{HomeStat, PokeLines};

/// How long a poke's line stays under the picture (`PokeablePicture.shownFor`).
const POKE_SHOWN: Duration = Duration::from_millis(2500);

/// A page's picture: the Swift app's own, built into the app so it needs no
/// files beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Art {
    Scheduled,
    Librarian,
}

impl Art {
    fn image(self) -> Arc<Image> {
        static SCHEDULED: OnceLock<Arc<Image>> = OnceLock::new();
        static LIBRARIAN: OnceLock<Arc<Image>> = OnceLock::new();
        match self {
            Art::Scheduled => SCHEDULED.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../../app/Sources/Threadbear/Resources/Art/scheduled.png").to_vec()))).clone(),
            Art::Librarian => LIBRARIAN.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../../app/Sources/Threadbear/Resources/Art/librarian.png").to_vec()))).clone(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Art::Scheduled => "The bear winding a big alarm clock, beside a calendar",
            Art::Librarian => "The librarian bear, filing a card in a card catalogue",
        }
    }

    /// Fitted into the 280 by 260 the Swift page gives it, keeping its shape:
    /// the alarm clock (765 by 822 pixels) is taller than wide, the card
    /// catalogue (732 by 676) wider.
    fn size(self) -> (f32, f32) {
        let (w, h) = match self {
            Art::Scheduled => (765., 822.),
            Art::Librarian => (732., 676.),
        };
        let scale = f32::min(280. / w, 260. / h);
        (w * scale, h * scale)
    }

    fn lines(self) -> &'static [&'static str] {
        match self {
            Art::Scheduled => PokeLines::SCHEDULED,
            Art::Librarian => PokeLines::LIBRARIAN,
        }
    }
}

/// What a page's picture is saying when it's poked (`PokeablePicture`): a
/// secret, so nothing hints at it and the pointer stays the arrow.
pub(crate) struct Pokes {
    lines: PokeLines,
    pub(crate) line: Option<&'static str>,
    /// Counts the pokes, so an old line's fade leaves a newer one alone.
    count: u64,
    fade: Option<Task<()>>,
}

impl Pokes {
    pub(crate) fn new(art: Art) -> Self {
        Pokes { lines: PokeLines::new(art.lines()), line: None, count: 0, fade: None }
    }
}

impl Threadbear {
    fn pokes_mut(&mut self, art: Art) -> &mut Pokes {
        match art {
            Art::Scheduled => &mut self.scheduled.pokes,
            Art::Librarian => &mut self.librarian_pokes,
        }
    }

    /// `HomePage`: the picture, the lines about the view, its numbers in
    /// tiles, and `below` under them.
    pub(crate) fn render_home_page(&mut self, art: Art, about: AnyElement, stats: &[HomeStat], below: Option<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        let line = self.pokes_mut(art).line;
        let (width, height) = art.size();
        let picture = div()
            .relative()
            .child(
                div()
                    .id("home-picture")
                    .test_support()
                    .aria_label(art.label())
                    .w(px(width))
                    .h(px(height))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.poke_home_picture(art, cx)))
                    .child(img(art.image()).size_full().object_fit(ObjectFit::Contain)),
            )
            // Drawn in the gap below, taking no room of its own.
            .when_some(line, |s, line| s.child(div().absolute().left_0().bottom(px(-24.)).whitespace_nowrap().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(line)));
        let tiles = (!stats.is_empty()).then(|| {
            // Three across at most, so six tiles make even rows.
            h_flex().flex_wrap().max_w(px(620.)).gap(px(14.)).children(stats.iter().enumerate().map(|(i, stat)| home_stat_tile(i, stat, cx)))
        });
        div()
            .id("home-page")
            .size_full()
            .overflow_y_scroll()
            .child(
                h_flex().w_full().justify_center().child(
                    v_flex()
                        .w_full()
                        .max_w(px(1000. + 64.))
                        .px(px(32.))
                        .pt(px(28.))
                        .pb(px(48.))
                        .gap(px(32.))
                        // Room under the picture for what it says when it's poked.
                        .child(v_flex().gap(px(36.)).child(picture).child(div().max_w(px(620.)).child(about)))
                        .children(tiles)
                        .children(below.map(|b| div().max_w(px(620.)).child(b))),
                ),
            )
            .into_any_element()
    }

    /// The picture says something when it's clicked, and it fades after a
    /// while (`PokeablePicture.poke`). The Swift app's wobble isn't here yet.
    pub(crate) fn poke_home_picture(&mut self, art: Art, cx: &mut Context<Self>) {
        let pokes = self.pokes_mut(art);
        let Some(line) = pokes.lines.poke() else { return };
        pokes.line = Some(line);
        pokes.count += 1;
        let count = pokes.count;
        cx.notify();
        let fade = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(POKE_SHOWN).await;
            this.update(cx, |this, cx| {
                let pokes = this.pokes_mut(art);
                if pokes.count == count {
                    pokes.line = None;
                    pokes.lines.end_run();
                    cx.notify();
                }
            })
            .ok();
        });
        self.pokes_mut(art).fade = Some(fade);
    }
}

/// `ModeHeader`: the view's name at the top of the sidebar, and its count on
/// the right when it has one.
pub(crate) fn mode_header(title: &'static str, count: Option<usize>, p: &Palette) -> impl IntoElement {
    h_flex()
        .flex_none()
        .h(px(34.))
        .pt(px(4.))
        // On the same column as the titles below.
        .pl(px(OUTER_INSET + LEADING))
        .pr(px(OUTER_INSET + TRAILING))
        .gap(px(6.))
        .items_center()
        .child(div().text_size(px(style::size::SIDEBAR_BUTTON)).line_height(px(17.)).font_semibold().text_color(p.fg).child(title))
        .child(div().flex_1())
        .children(count.filter(|c| *c > 0).map(|count| div().text_size(px(13.)).line_height(px(16.)).text_color(p.faint).child(count.to_string())))
}

/// `HomeTitle`: the page's title, with more behind the ⓘ, and the line under it.
pub(crate) fn home_title(title: &'static str, line: &'static str, more: &'static str, cx: &App) -> impl IntoElement {
    let secondary = style::secondary_text(cx);
    v_flex()
        .gap(px(12.))
        .child(h_flex().items_center().gap(px(8.)).child(div().text_size(px(24.)).line_height(px(29.)).font_semibold().child(title)).child(info_tip("home-more", more, cx)))
        .child(div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(line))
}

/// `InfoTip`: an ⓘ that says more in its tooltip, and in a popover when
/// clicked, for text too long to read in a tooltip.
pub(crate) fn info_tip(id: impl Into<SharedString>, text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let secondary = style::secondary_text(cx);
    let (id, text): (SharedString, SharedString) = (id.into(), text.into());
    let shown = text.clone();
    Popover::new(ElementId::Name(id.clone()))
        // Above it, as the Swift popover's arrow on its bottom edge puts it.
        .anchor(Anchor::BottomLeft)
        .trigger(InfoTrigger { id, text, color: secondary })
        .content(move |_, _, _| div().w(px(280.)).p(px(12.)).text_size(px(12.)).child(shown.clone()))
        .into_any_element()
}

#[derive(IntoElement)]
struct InfoTrigger {
    id: SharedString,
    text: SharedString,
    color: Hsla,
}

impl gpui_kit::component::Selectable for InfoTrigger {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for InfoTrigger {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        // A little bigger than the icon, as it's small to aim at.
        crate::highlights::tipped(div().id(SharedString::from(format!("{}-button", self.id))).p(px(2.)).cursor_pointer(), self.text).child(icon("info.circle", 11., self.color))
    }
}

/// `HomeStatusLine`: what it's doing now, under the title; a dot, accent while busy.
pub(crate) fn home_status_line(text: &str, busy: bool, cx: &App) -> impl IntoElement {
    let dot = if busy { style::control_accent(cx) } else { style::faint_text(cx) };
    h_flex()
        .pt(px(4.))
        .gap(px(6.))
        .items_center()
        .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
        .child(div().truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).child(text.to_owned()))
}

/// `HomeStatTile`: one number, what it counts, and more in its tooltip.
fn home_stat_tile(index: usize, stat: &HomeStat, cx: &App) -> impl IntoElement {
    let secondary = style::secondary_text(cx);
    let fill = style::primary_opacity(cx, 0.04);
    crate::highlights::tipped(div().id(("home-stat", index)).flex_grow(1.).flex_basis(px(170.)).min_w(px(170.)), stat.help.clone()).child(
        v_flex()
            .gap(px(4.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(10.))
            .bg(fill)
            .child(div().truncate().text_size(px(22.)).line_height(px(26.)).font_semibold().child(stat.value.clone()))
            .child(div().truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(stat.label.clone())),
    )
}
