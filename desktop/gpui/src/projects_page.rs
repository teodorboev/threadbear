// The Projects page, what Projects in the activity bar shows in the main panel
// (a port of the Swift app's ProjectsHome): a card per project, newest activity
// first, with its icon, what its threads are doing and where it's up to, and a
// search over what each is about. A card opens the project's page; the
// sidebar's Projects view lists the threads.
use crate::app::Threadbear;
use crate::projects::project_icon;
use crate::style::{self, px};
use crate::{icons, now_ms};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use threadbear_core::projects::{self as rules, CardPart, CardStatus, Project, Sections};
use crate::tips::Tip as _;

/// Cards are this wide at least and at most (`GridItem(.adaptive(minimum:
/// 240, maximum: 340))`), this far apart across and down.
const CARD_MIN: f32 = 240.;
const CARD_MAX: f32 = 340.;
const CARD_GAP: f32 = 20.;
const ROW_GAP: f32 = 24.;

/// A card, worked out before it's drawn.
struct CardView {
    project: Project,
    status: CardStatus,
    last_activity_ms: i64,
    parts: Vec<String>,
}

thread_local! {
    /// The cards' column as last laid out, so the grid can be the Swift
    /// one's: as many columns as fit, all one width. A frame behind, which
    /// only shows on the first one.
    static GRID_WIDTH: Rc<Cell<f32>> = Rc::new(Cell::new(0.));
}

impl Threadbear {
    /// The page's search box, made the first time it's asked for. Return opens
    /// the top card, Escape clears it.
    pub(crate) fn projects_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some((search, _)) = &self.projects.search {
            return search.clone();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let events = cx.subscribe(&search, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => {
                let first = this.shown_project_cards(cx).into_iter().next();
                if let (Some((_, id)), true) = (first, this.projects_query(cx).is_some()) {
                    this.open_project_page(&id, cx);
                }
            }
            _ => {}
        });
        self.projects.search = Some((search.clone(), events));
        search
    }

    /// What's typed in the search, if anything.
    fn projects_query(&self, cx: &App) -> Option<String> {
        let (search, _) = self.projects.search.as_ref()?;
        let query = search.read(cx).value().trim().to_owned();
        (!query.is_empty()).then_some(query)
    }

    /// The cards the search leaves, in order: (title, id).
    pub(crate) fn shown_project_cards(&self, cx: &App) -> Vec<(String, String)> {
        let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
        let cards = rules::cards(&sections, 3, self.project_decisions_on());
        rules::matching(cards, &self.projects_query(cx).unwrap_or_default()).into_iter().map(|c| (c.project.title.clone(), c.project.id.clone())).collect()
    }

    /// `ProjectsHome`: the title, a summary, the search and New project, then
    /// the cards.
    pub(crate) fn render_projects_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.projects_search(window, cx);
        // Typing finds a project from the moment the page is up, as the Swift
        // field's focusOnAppear; not over anything that's taken the keys.
        // Leaves AppKit's keyboard where it is: a drawing's, not a request.
        if window.focused(cx).is_none_or(|f| f == self.root_focus) {
            let handle = search.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        let decisions = self.project_decisions_on();
        let query = self.projects_query(cx);
        let (all, total, summary) = {
            let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
            let all = rules::cards(&sections, 3, decisions);
            let total = all.len();
            let summary = match &query {
                Some(_) => None,
                None => Some(rules::summary(total, &rules::overall(&sections, decisions))),
            };
            let shown: Vec<CardView> = rules::matching(all, query.as_deref().unwrap_or(""))
                .into_iter()
                .map(|c| CardView { project: c.project.clone(), status: c.status, last_activity_ms: c.last_activity_ms, parts: c.parts.iter().map(|p| p.title.clone()).collect() })
                .collect();
            (shown, total, summary)
        };
        let summary = summary.unwrap_or_else(|| rules::match_count(all.len(), total));
        let faint = style::faint_text(cx);
        let secondary = style::secondary_text(cx);
        let busy = self.projects.busy.contains("new");
        // On the title's baseline, as the Swift row lines them up
        // (`firstTextBaseline`): the summary, the search box and the button
        // sit lower than the title's top by what their text is smaller.
        let header = h_flex()
            .w_full()
            .items_start()
            .gap(px(10.))
            .child(div().flex_none().text_size(px(24.)).line_height(px(29.)).font_semibold().child("Projects"))
            // In a narrow window the summary is what gives way: one line cut
            // short, rather than the search's width jumping with each key.
            .child(div().flex_1().min_w_0().mt(px(10.)).truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(faint).child(summary))
            .child(div().mt(px(4.)).child(self.render_projects_search(&search, cx)))
            .child(div().mt(px(6.)).child(icon_push_button("projects-new", "plus", "New project", "Make a new project", busy, cx, cx.listener(|this, _: &ClickEvent, window, cx| this.new_project(None, window, cx)))));
        let body: AnyElement = if total == 0 {
            div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child("No projects yet. The librarian makes them as threads run, or you can make one.").into_any_element()
        } else if all.is_empty() {
            div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child("No projects match that.").into_any_element()
        } else {
            self.render_project_cards(all, cx)
        };
        div()
            .id("projects-home")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.projects.home_scroll)
            .child(h_flex().w_full().justify_center().child(v_flex().w_full().max_w(px(1160. + 64.)).px(px(32.)).pt(px(28.)).pb(px(48.)).gap(px(22.)).child(header).child(body)))
            .into_any_element()
    }

    /// `SearchField`: the glass, then the box, on a faint fill with a hairline.
    fn render_projects_search(&mut self, search: &Entity<InputState>, cx: &mut Context<Self>) -> AnyElement {
        let faint = style::faint_text(cx);
        let clear = search.clone();
        div()
            .id("projects-search")
            .tip("Search projects")
            .flex_shrink(1.)
            .min_w(px(120.))
            .w(px(240.))
            .h(px(28.))
            .pl(px(9.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(7.))
            .bg(style::primary_opacity(cx, 0.035))
            .border(gpui_kit::px(0.5))
            .border_color(cx.theme().border)
            // Escape clears it (`onExitCommand`).
            .capture_key_down(cx.listener(move |_, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    clear.update(cx, |s, cx| s.set_value("", window, cx));
                    cx.stop_propagation();
                }
            }))
            .child(icons::symbol("magnifyingglass").with_size(px(12.)).text_color(faint))
            .child(div().flex_1().min_w_0().child(Input::new(search).appearance(false).small().cleanable(true).pl(px(0.))))
            .into_any_element()
    }

    /// The cards in a grid as LazyVGrid lays out an adaptive column: as many
    /// as fit at the narrowest, each the same width up to the widest.
    fn render_project_cards(&mut self, cards: Vec<CardView>, cx: &mut Context<Self>) -> AnyElement {
        let measured = GRID_WIDTH.with(|w| w.clone());
        let width = measured.get().max(CARD_MIN);
        let columns = (((width + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor() as usize).max(1);
        let card_width = ((width - CARD_GAP * (columns as f32 - 1.)) / columns as f32).min(CARD_MAX);
        let mut grid = h_flex().w_full().flex_wrap().items_start().gap_x(px(CARD_GAP)).gap_y(px(ROW_GAP));
        for card in cards {
            grid = grid.child(self.render_project_card(card, card_width, cx));
        }
        let probe = measured.clone();
        let notify = cx.entity().downgrade();
        div()
            .relative()
            .w_full()
            .child(grid)
            // The width it was given, for the next frame's columns; a change
            // draws again so a resize settles at once.
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let now = f32::from(bounds.size.width) / style::zoomed(1.);
                        if (probe.get() - now).abs() > 0.5 {
                            probe.set(now);
                            notify.update(cx, |_, cx| cx.notify()).ok();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }

    /// `ProjectCardView`: its icon on a wide tile with its open threads'
    /// count in the corner, then its title, what its threads are doing, its
    /// parts and the first lines of its state. A click opens its page; its
    /// menu is the project's.
    fn render_project_card(&mut self, card: CardView, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = card.project.id.clone();
        let secondary = style::secondary_text(cx);
        let art = self.render_project_art(&card.project, card.status.open, width, cx);
        let mut words = v_flex()
            .gap(px(5.))
            .px(px(2.))
            .child(div().truncate().text_size(px(15.)).line_height(px(19.)).font_semibold().child(card.project.title.clone()))
            .child(project_status_line(&card.status, card.last_activity_ms, cx));
        // Its parts are inside its card rather than cards of their own; its
        // page lists them with their state.
        if !card.parts.is_empty() {
            words = words.child(
                h_flex()
                    .gap(px(4.))
                    .items_center()
                    .text_size(px(style::size::SIDEBAR_META + 1.))
                    .line_height(px(15.))
                    .text_color(secondary)
                    .child(icons::symbol("square.stack").with_size(px(11.)).text_color(secondary))
                    .child(div().min_w_0().truncate().child(card.parts.join(" · "))),
            );
        }
        if !card.project.state.is_empty() {
            words = words.child(div().pt(px(2.)).text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).line_clamp(3).child(rules::first_lines(&card.project.state)));
        }
        let (open_id, menu_id) = (id.clone(), id.clone());
        crate::highlights::tipped(div().id(gpui_kit::SharedString::from(format!("project-card-{id}"))).w(px(width)).flex_none().cursor_pointer(), format!("Open {}", card.project.title))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_project_page(&open_id, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| this.open_project_menu(&menu_id, ev.position, window, cx)))
            .child(v_flex().gap(px(10.)).child(art).child(words))
            .into_any_element()
    }

    /// `ProjectArt`: the project's icon in the middle of a wide tile, and how
    /// many open threads it has in the corner. An icon that's a picture also
    /// fills the tile behind it, blurred, so each card takes its colour from
    /// its own icon; a symbol or a letter sits on a quiet grey.
    fn render_project_art(&mut self, project: &Project, open: usize, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let path = project.icon.as_ref().and_then(|i| i.image_path.clone());
        let picture = path.is_some();
        let backdrop = path.and_then(|path| self.card_backdrop(&path, cx));
        let shadow = if picture { vec![BoxShadow { color: hsla(0., 0., 0., 0.25), offset: point(px(0.), px(3.)), blur_radius: px(8.), spread_radius: px(0.), inset: false }] } else { vec![] };
        let mut tile = div()
            .relative()
            .w(px(width))
            .h(px(width / 2.))
            .rounded(px(12.))
            .overflow_hidden()
            .bg(style::primary_opacity(cx, 0.05))
            .border_1()
            .border_color(style::primary_opacity(cx, 0.08))
            .flex()
            .items_center()
            .justify_center()
            .when_some(backdrop, |s, backdrop| s.child(img(backdrop).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Fill).opacity(0.55)))
            .child(div().rounded(px(56. * 0.22)).shadow(shadow).child(project_icon(Some(project), "", 56., cx)));
        if open > 0 {
            tile = tile.child(div().absolute().top(px(8.)).right(px(8.)).child(thread_count_badge(open, &project.id, cx)));
        }
        tile.into_any_element()
    }

    /// A picture icon's blurred copy for its card, made once per path in the
    /// background (each copy the runner makes has its own name, so a changed
    /// icon is a new path and nothing here goes stale).
    fn card_backdrop(&mut self, path: &str, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        if let Some(made) = self.projects.backdrops.get(path) {
            return made.clone();
        }
        self.projects.backdrops.insert(path.to_owned(), None);
        let path = path.to_owned();
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let made = cx.background_executor().spawn(async move { blurred_backdrop(&file) }).await;
            if made.is_some() {
                this.update(cx, |this, cx| {
                    this.projects.backdrops.insert(path, made);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        None
    }

    /// The Projects page's cards in the dump (`projectCards`), while it's
    /// showing: "Title: 1 needs you · 2 working", or "Title: quiet", with
    /// its parts after.
    #[cfg(feature = "automation")]
    pub(crate) fn project_cards_dump(&self) -> Option<Vec<String>> {
        if self.open_project().is_some() || self.home_page() != Some(crate::shell::SidebarMode::Projects) {
            return None;
        }
        let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
        let cards = rules::cards(&sections, 3, self.project_decisions_on());
        Some(
            cards
                .iter()
                .map(|card| {
                    let parts = if card.parts.is_empty() { String::new() } else { format!(" (parts: {})", card.parts.iter().map(|p| p.title.as_str()).collect::<Vec<_>>().join(", ")) };
                    format!("{}: {}{parts}", card.project.title, card.status.words())
                })
                .collect(),
        )
    }
}

/// The picture as the Swift tile fills with it (scaled to cover a 2 by 1
/// tile, so the middle of a square one shows), blurred by about what its
/// 28 pt is across a 340 pt card. Small: drawing it stretched smooths it more.
fn blurred_backdrop(path: &str) -> Option<Arc<Image>> {
    use image::imageops::{self, FilterType};
    let picture = image::open(path).ok()?.to_rgba8();
    let (w, h) = picture.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (cw, ch) = if w >= h * 2 { (h * 2, h) } else { (w, (w / 2).max(1)) };
    let band = imageops::crop_imm(&picture, (w - cw) / 2, (h - ch) / 2, cw, ch).to_image();
    let small = imageops::resize(&band, 96, 48, FilterType::Triangle);
    let blurred = imageops::blur(&small, 8.);
    let mut png = Vec::new();
    blurred.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(Arc::new(Image::from_bytes(gpui_kit::ImageFormat::Png, png)))
}

/// `ThreadCount`: how many open threads it has, in the tile's corner.
fn thread_count_badge(count: usize, id: &str, cx: &App) -> AnyElement {
    let secondary = style::secondary_text(cx);
    crate::highlights::tipped(
        h_flex()
            .id(gpui_kit::SharedString::from(format!("project-count-{id}")))
            .gap(px(4.))
            .items_center()
            .px(px(7.))
            .py(px(3.))
            .rounded_full()
            .bg(cx.theme().popover.opacity(0.85))
            .text_size(px(style::size::SIDEBAR_META))
            .line_height(px(14.))
            .font_medium()
            .text_color(secondary),
        rules::open_threads_tip(count),
    )
    .child(icons::symbol("bubble.left.and.bubble.right").with_size(px(11.)).text_color(secondary))
    .child(count.to_string())
    .into_any_element()
}

/// `StatusLine`: what its threads are doing, most pressing first, in the
/// sidebar's colours (orange waiting on Mike, the accent and its spinner
/// working); a quiet project says when it was last active instead.
fn project_status_line(status: &CardStatus, last_activity_ms: i64, cx: &App) -> AnyElement {
    let faint = style::faint_text(cx);
    let line = h_flex().gap(px(5.)).items_center().text_size(px(style::size::SIDEBAR_META + 1.)).line_height(px(15.)).font_medium().whitespace_nowrap().overflow_hidden();
    if status.is_quiet() {
        let when = threadbear_core::sidebar::relative_short(last_activity_ms, &threadbear_core::sidebar::local_now());
        return line.child(div().text_color(faint).child(format!("Last active {when}"))).into_any_element();
    }
    let mut line = line;
    for (n, part) in status.parts().into_iter().enumerate() {
        if n > 0 {
            line = line.child(div().text_color(faint).child("·"));
        }
        let color = match part {
            CardPart::NeedsYou(_) | CardPart::Decisions(_) => style::needs_you(cx),
            CardPart::Working(_) => style::control_accent(cx),
            CardPart::Failed(_) => style::failed(cx),
            CardPart::Unread(_) => style::secondary_text(cx),
        };
        if matches!(part, CardPart::Working(_)) {
            line = line.child(div().flex_none().size(px(11.)).child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(color).with_size(px(11.))));
        }
        line = line.child(div().text_color(color).child(part.text()));
    }
    line.into_any_element()
}

/// A bordered push button with a symbol before its words (SwiftUI's `Button`
/// with a `Label`, default style), as push_button.rs draws one without: 24
/// tall, a 13 pt label, a fill of 8% white or black. Spins and takes no
/// clicks while `busy`.
pub(crate) fn icon_push_button(id: &'static str, symbol: &'static str, label: &'static str, tip: &'static str, busy: bool, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let color = style::label_text(cx);
    let ink = if cx.theme().is_dark() { 1. } else { 0. };
    let mark: AnyElement = if busy { Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(color).with_size(px(11.)).into_any_element() } else { icons::symbol(symbol).with_size(px(12.)).text_color(color).into_any_element() };
    let button = div()
        .id(id)
        .flex_none()
        .h(px(24.))
        .px(px(10.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(px(13.))
        .line_height(px(style::natural_line(13.)))
        .whitespace_nowrap()
        .text_color(color)
        .bg(hsla(0., 0., ink, 0.08))
        .active(move |s| s.bg(hsla(0., 0., ink, 0.16)))
        .when(busy, |s| s.opacity(0.6))
        .when(!busy, |s| s.on_click(on_click))
        .child(mark)
        .child(label);
    crate::highlights::tipped(button, tip).into_any_element()
}
