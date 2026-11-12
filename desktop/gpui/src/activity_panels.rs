// What the activity bar's own panels share (runners_panel.rs, updates_panel.rs,
// connectors_panel.rs): the view each sits in, which draws only when the app
// changes, as the Threads panel's does, and hears when its panel comes on show
// or goes, for the panels that ask the runner while they're open (the Swift
// views' `.task` and `.onAppear`); and the small controls they're made of, the
// Swift app's borderless icon buttons, its ⓘ popovers and its push buttons.
use crate::app::Threadbear;
use crate::highlights::tipped;
use crate::{icons, shell, style};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;

/// One of the activity bar's panels, as the dock shows it: drawn again only
/// when the app changes, so scrolling the transcript beside it doesn't redraw
/// it every frame.
pub(crate) struct PanelView {
    app: WeakEntity<Threadbear>,
    panel: &'static str,
    showing: bool,
    _subs: Vec<Subscription>,
}

impl PanelView {
    /// Made while the app is being updated (it draws the dock), so it's handed
    /// the dock rather than reading the app for it.
    pub(crate) fn new(app: &Entity<Threadbear>, dock: &Entity<threadbear_dock::DockModel>, panel: &'static str, cx: &mut Context<Self>) -> Self {
        let app_changed = cx.observe(app, |_, _, cx| cx.notify());
        // On show and gone again, as the dock has it: a tab behind another
        // isn't on show, as the Swift view isn't drawn there either.
        let dock_changed = cx.observe(dock, |this: &mut Self, dock, cx| {
            let showing = dock.read(cx).controller.is_showing(this.panel);
            if showing != this.showing {
                this.showing = showing;
                this.tell_app(showing, cx);
            }
        });
        let mut view = Self { app: app.downgrade(), panel, showing: false, _subs: vec![app_changed, dock_changed] };
        // Made the first time it's drawn, which is it coming on show.
        view.showing = true;
        view.tell_app(true, cx);
        view
    }

    fn tell_app(&self, showing: bool, cx: &mut Context<Self>) {
        let (app, panel) = (self.app.clone(), self.panel);
        // Afterwards: this can come from inside the app's own change to the dock.
        cx.defer(move |cx| {
            if let Some(app) = app.upgrade() {
                app.update(cx, |t, cx| t.activity_panel_shown(panel, showing, cx));
            }
        });
    }
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.panel;
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.render_activity_panel(panel, window, cx)),
            None => div().into_any_element(),
        }
    }
}

impl Threadbear {
    /// What one of these panels shows, in its cached view (a drag's ghost of
    /// it is drawn in place, as the Threads panel's is).
    pub(crate) fn activity_panel_view(&mut self, panel: &'static str, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if ghost {
            return self.render_activity_panel(panel, window, cx);
        }
        let (app, dock) = (cx.entity(), self.dock.clone());
        let slot = match panel {
            shell::RUNNERS => &mut self.runners_panel.view,
            shell::UPDATES => &mut self.updates_panel.view,
            _ => &mut self.connectors_panel.view,
        };
        let view = slot.get_or_insert_with(|| cx.new(|cx| PanelView::new(&app, &dock, panel, cx))).clone();
        AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
    }

    pub(crate) fn render_activity_panel(&mut self, panel: &'static str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match panel {
            shell::RUNNERS => self.render_runners_panel(window, cx),
            shell::UPDATES => self.render_updates_panel(window, cx),
            _ => self.render_connectors_panel(window, cx),
        }
    }

    /// A panel came on show or went.
    pub(crate) fn activity_panel_shown(&mut self, panel: &'static str, showing: bool, cx: &mut Context<Self>) {
        match panel {
            shell::RUNNERS => self.runners_panel_shown(showing, cx),
            shell::CONNECTORS if showing => self.refresh_connectors(false, cx),
            _ => {}
        }
    }

    /// Whether one of these panels is on show, for its button in the bar.
    pub(crate) fn activity_panel_showing(&self, panel: &str, cx: &App) -> bool {
        self.dock.read(cx).controller.is_showing(panel)
    }
}

/// The panel's scrolling column: 16 points in from its edges, as the Swift
/// panels' `.padding(zoomed(16))`.
pub(crate) fn panel_column(id: &'static str, gap: f32, children: Vec<AnyElement>) -> AnyElement {
    div().id(id).size_full().overflow_y_scroll().child(v_flex().w_full().p(px(16.)).gap(px(gap)).children(children)).into_any_element()
}

/// A line of text with SwiftUI's own line height for its size.
pub(crate) fn text_line(text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    div().text_size(px(size)).line_height(px(style::natural_line(size))).text_color(color).child(text.into())
}

/// The panel's small grey heading ("App", "bruce"): 11 points, medium, in
/// the tertiary colour.
pub(crate) fn small_heading(text: impl Into<SharedString>, cx: &App) -> Div {
    text_line(text, 11., style::tertiary_label(cx)).font_weight(FontWeight::MEDIUM)
}

/// A borderless icon button in the secondary colour (`.buttonStyle(.borderless)`
/// with an 11 point symbol, which Lucide's thinner drawings match at 12), and
/// a spinner in its place while `busy`, taking no clicks (`.busy`).
pub(crate) fn icon_button(id: impl Into<SharedString>, symbol: &'static str, tip: impl Into<SharedString>, busy: bool, cx: &mut Context<Threadbear>, on_click: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static) -> AnyElement {
    let (secondary, fg) = (style::secondary_text(cx), style::label_text(cx));
    let id: SharedString = id.into();
    let el = tipped(div().id(id).flex_none().size(px(14.)).flex().items_center().justify_center().text_color(secondary), tip);
    if busy {
        return el.child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(secondary).with_size(px(12.))).into_any_element();
    }
    el.cursor_pointer()
        .hover(move |s| s.text_color(fg))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
        .child(icons::symbol(symbol).with_size(px(12.)))
        .into_any_element()
}

/// The height of a `.controlSize(.small)` push button, which a line of text
/// beside it sits 2 points down from, its baseline on the button's (a
/// service's name beside Connect, a CLI's version beside Update).
pub(crate) const SMALL_BUTTON: f32 = 20.;

/// A push button with its words, drawn as SwiftUI's default bordered one in
/// a panel (a flat rounded box a shade off the panel, no edge, the words in
/// the label colour), with its tooltip, which is also how scripts find it.
/// `small` is `.controlSize(.small)`: 20 points high with 11 point words.
/// While `busy` it's a spinner in its place and takes no clicks (`.busy`).
pub(crate) fn push_button(id: impl Into<SharedString>, label: impl Into<SharedString>, tip: impl Into<SharedString>, small: bool, busy: bool, cx: &mut Context<Threadbear>, on_click: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static) -> AnyElement {
    // As wide as the Swift buttons, measured side by side (59 points for a
    // small Update, 56 for Copy).
    let (height, pad, size, corner) = if small { (SMALL_BUTTON, 10., 11., 6.) } else { (24., 12., 13., 7.) };
    let id: SharedString = id.into();
    let el = tipped(
        div()
            .id(id)
            .relative()
            .flex_none()
            .h(px(height))
            .px(px(pad))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(corner))
            .whitespace_nowrap()
            .text_size(px(size))
            .line_height(px(style::natural_line(size)))
            .text_color(style::label_text(cx)),
        tip,
    );
    let label: SharedString = label.into();
    if busy {
        // Its room kept, the words hidden, under the spinner.
        return el
            .child(div().opacity(0.).child(label))
            .child(div().absolute().inset_0().flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(style::secondary_text(cx)).with_size(px(12.))))
            .into_any_element();
    }
    el.bg(push_fill(cx))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
        .child(label)
        .into_any_element()
}

/// The borderless menu's indicator as SwiftUI draws it beside 11 point words:
/// a chevron 7 points wide with a stroke of about 1.6, as bright as the
/// words (measured on the Swift app's pictures). Lucide's is a hairline at
/// this size, so it read as grey.
const MENU_CHEVRON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="3.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9.5l6 6 6-6"/></svg>"#;

/// A borderless pop-up menu's label (`.menuStyle(.borderlessButton)` with
/// its indicator): its words and a small chevron, no box, one line high,
/// for `dropdown_menu` to open a menu from.
#[derive(IntoElement)]
pub(crate) struct MenuLabel {
    inner: Stateful<Div>,
}

impl MenuLabel {
    pub(crate) fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, size: f32, color: Hsla) -> Self {
        let line = style::natural_line(size);
        let inner = div()
            .id(id)
            .flex_none()
            .h(px(line))
            // The Swift menu's words start 3 points in from its edge, where
            // plain lines start at the edge in both apps.
            .pl(px(3.))
            .flex()
            .items_center()
            // The chevron 4 points after the words, as Swift's.
            .gap(px(2.))
            .cursor_pointer()
            .text_size(px(size))
            .line_height(px(line))
            .text_color(color)
            .child(label.into())
            .child(gpui_kit::component::Icon::default().data(MENU_CHEVRON).with_size(px(size - 1.)).text_color(color));
        Self { inner }
    }
}

impl RenderOnce for MenuLabel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.inner
    }
}

impl Styled for MenuLabel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl InteractiveElement for MenuLabel {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl gpui_kit::component::Selectable for MenuLabel {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl gpui_kit::component::menu::DropdownMenu for MenuLabel {}

/// Words that work as a button, in the accent (`.buttonStyle(.link)`), or
/// grey and borderless ("Check now"). Spins in place while `busy`.
pub(crate) fn text_button(id: impl Into<SharedString>, label: impl Into<SharedString>, tip: impl Into<SharedString>, size: f32, link: bool, busy: bool, cx: &mut Context<Threadbear>, on_click: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static) -> AnyElement {
    let color = if link { style::link_text(cx) } else { style::secondary_text(cx) };
    let fg = if link { color } else { style::label_text(cx) };
    let id: SharedString = id.into();
    let el = tipped(div().id(id).relative().min_w_0().truncate().text_size(px(size)).line_height(px(style::natural_line(size))).text_color(color), tip).child(label.into());
    if busy {
        // The words keep their room, hidden, under the spinner.
        return el
            .text_color(color.opacity(0.))
            .child(div().absolute().inset_0().flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(style::secondary_text(cx)).with_size(px(12.))))
            .into_any_element();
    }
    el.cursor_pointer()
        .hover(move |s| if link { s.underline().text_decoration_color(color) } else { s.text_color(fg) })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
        .into_any_element()
}

/// The ⓘ: says what the panel shows in a popover under it, a paragraph a
/// line, 280 points wide (`RunnersAbout`, `ConnectorsAbout`). Words in
/// backticks are drawn as code, as SwiftUI's Markdown draws them.
pub(crate) fn about_button(id: &'static str, paragraphs: Vec<String>, footnote: Option<String>, cx: &App) -> AnyElement {
    let secondary = style::secondary_text(cx);
    let fg = style::label_text(cx);
    Popover::new(ElementId::Name(id.into()))
        .anchor(Anchor::TopRight)
        .trigger(AboutTrigger { id, color: secondary })
        .content(move |_, _, cx| {
            let mono = cx.theme().mono_font_family.clone();
            v_flex()
                .w(px(280.))
                .p(px(2.))
                .gap(px(8.))
                .text_size(px(12.))
                .line_height(px(style::natural_line(12.)))
                .text_color(fg)
                .children(paragraphs.iter().map(|p| code_runs(p, mono.clone())))
                .children(footnote.clone().map(|f| div().text_color(secondary).child(f)))
        })
        .into_any_element()
}

/// A paragraph with its `code` drawn in the monospaced font.
fn code_runs(text: &str, mono: SharedString) -> AnyElement {
    if !text.contains('`') {
        return div().child(text.to_owned()).into_any_element();
    }
    let mut row = div().flex().flex_wrap();
    for (i, piece) in text.split('`').enumerate() {
        if piece.is_empty() {
            continue;
        }
        // Each word its own piece, so the paragraph still wraps between words.
        for word in piece.split_inclusive(' ') {
            let el = div().whitespace_nowrap().child(word.replace(' ', "\u{a0}"));
            row = row.child(if i % 2 == 1 { el.font_family(mono.clone()) } else { el });
        }
    }
    row.into_any_element()
}

#[derive(IntoElement)]
struct AboutTrigger {
    id: &'static str,
    color: Hsla,
}

impl gpui_kit::component::Selectable for AboutTrigger {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for AboutTrigger {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let fg = style::label_text(cx);
        tipped(div().id(SharedString::from(format!("{}-button", self.id))).flex_none().size(px(14.)).flex().items_center().justify_center().cursor_pointer().text_color(self.color).hover(move |s| s.text_color(fg)), "What this panel shows")
            .child(icons::symbol("info.circle").with_size(px(11.)))
    }
}

/// A small dot in a line's leading gutter (a runner's state, a service's).
pub(crate) fn dot(color: Hsla) -> Div {
    div().flex_none().size(px(6.)).rounded_full().bg(color)
}

/// `Color.green` and `Color.orange` as SwiftUI draws them in each look.
pub(crate) fn system_green(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        Hsla::from(Rgba { r: 0.188, g: 0.82, b: 0.345, a: 1. })
    } else {
        Hsla::from(Rgba { r: 0.204, g: 0.78, b: 0.349, a: 1. })
    }
}

/// A SwiftUI push button's fill in a panel: white at 7.5% on dark, black at
/// 7.5% on light (#2d2d2d on the dark panel's #1c1c1c, #ececec on white,
/// measured on the Swift app's pictures). Not the composer card's
/// `style::button_fill`, whose buttons have an edge.
fn push_fill(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        hsla(0., 0., 1., 0.075)
    } else {
        hsla(0., 0., 0., 0.075)
    }
}

pub(crate) fn system_orange(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        Hsla::from(Rgba { r: 1., g: 0.624, b: 0.039, a: 1. })
    } else {
        Hsla::from(Rgba { r: 1., g: 0.584, b: 0., a: 1. })
    }
}

/// A row of buttons, 8 points apart, with a spinner after them while one of
/// them is on its way.
pub(crate) fn button_row(buttons: Vec<AnyElement>, spinning: bool, cx: &App) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .items_center()
        .children(buttons)
        .when(spinning, |s| s.child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(style::secondary_text(cx)).with_size(px(11.))))
        .into_any_element()
}
