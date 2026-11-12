// The Usage panel and the activity bar's honey pot, a port of the Swift app's
// UsagePanel.swift and HoneyPot.swift. At the top of the panel, what's been
// spent: today's tokens, the last hour's and the week's, and a chart by the
// hour or the day with each login its own colour. Then a card per pool, each
// login in it with its 5-hour and weekly room, how fast the week is going and
// whether it lasts, and a model's own week (Fable's), which a Settings card
// at the foot can hide. The numbers and words are worked out in
// threadbear_core::usage; this draws them.
//
// The limits come with the runner's logins (hello, then `accounts.updated`
// and `pools.updated`); the tokens and the readings over time from
// `usage.stats`, asked for when the panel opens and once a minute while it's
// on screen. Nothing here sends anything to the runner, so the only clicks
// (the icon, the chart's range, the Settings switch) show at once.
use crate::app::Threadbear;
use crate::{call, icons, now_ms, shell, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use threadbear_core::chrono::Local;
use threadbear_core::logins::Account;
use threadbear_core::usage::{self as rules, Card, Chart, LoginRow, LoginStatus, Meter, Range, Stats, Tile, Tone};
use crate::tips::Tip as _;

/// How often the panel asks again while it's open, and how often it checks
/// it still is.
const REFRESH: Duration = Duration::from_secs(60);
const CHECK: Duration = Duration::from_secs(1);

/// The Usage panel's and the icon's state (`Threadbear::usage`).
pub(crate) struct UsageState {
    /// `usage.stats`'s last answer.
    pub(crate) stats: Option<Stats>,
    /// `usageStatsSupported`: None until the runner has answered, false from
    /// one older than `usage.stats` (its spend isn't shown, and it isn't asked
    /// again).
    pub(crate) supported: Option<bool>,
    /// The chart's range, remembered as `usage.chartRange`.
    pub(crate) range: Range,
    /// The Settings card's switch, remembered as `usage.showModelWeeks`: on
    /// unless it's been turned off, when the logins leave out a model's own
    /// week ("Weekly Fable").
    pub(crate) show_model_weeks: bool,
    /// Asking while the panel's on screen.
    poll: Option<Task<()>>,
    /// The chart's slot under the pointer.
    hovered: Option<i64>,
    /// The chart's plot, where it was last drawn, for the pointer.
    plot: Rc<Cell<Option<Bounds<Pixels>>>>,
    scroll: ScrollHandle,
    /// The panel's width when it was last drawn: SwiftUI's ViewThatFits picks
    /// the meters' layout and whether a tile's line fits from it.
    width: Rc<Cell<f32>>,
    pot: Pot,
    /// The panel as a view of its own, made the first time it's shown.
    pub(crate) view: Option<Entity<UsagePanelView>>,
}

/// The Usage panel as a view of its own, so the dock reuses its last frame
/// (`cached` in shell.rs), as SidebarView does. A spinner anywhere redraws
/// the window every frame, and drawn in with the shell the panel was laid out
/// and painted again each time, charts and all: with it open wide, a debug
/// build spent so long on frames that it stopped answering. Everything it
/// draws is the app's state, so it draws again whenever the app notifies, and
/// when it scrolls or changes size.
pub(crate) struct UsagePanelView {
    app: WeakEntity<Threadbear>,
    _app_changed: Subscription,
}

impl UsagePanelView {
    pub(crate) fn new(app: &Entity<Threadbear>, cx: &mut Context<Self>) -> Self {
        let app_changed = cx.observe(app, |_, _, cx| cx.notify());
        Self { app: app.downgrade(), _app_changed: app_changed }
    }
}

impl Render for UsagePanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.render_usage_panel(false, window, cx)),
            None => div().into_any_element(),
        }
    }
}

impl UsageState {
    pub(crate) fn new(range: Option<&str>, show_model_weeks: Option<bool>) -> Self {
        UsageState {
            stats: None,
            supported: None,
            range: range.and_then(Range::from_name).unwrap_or_default(),
            show_model_weeks: show_model_weeks.unwrap_or(true),
            poll: None,
            hovered: None,
            plot: Rc::new(Cell::new(None)),
            scroll: ScrollHandle::new(),
            width: Rc::new(Cell::new(0.)),
            pot: Pot::default(),
            view: None,
        }
    }
}

impl Threadbear {
    /// The panel's charts (`loadUsageStats`): asked for when the panel opens
    /// and once a minute while it's on screen, as the Swift panel's task does;
    /// the task ends when the panel goes.
    fn keep_usage_fresh(&mut self, cx: &mut Context<Self>) {
        if self.usage.poll.is_some() || self.usage.supported == Some(false) {
            return;
        }
        self.usage.poll = Some(cx.spawn(async move |this, cx| {
            let mut asked: Option<Instant> = None;
            loop {
                if asked.is_none_or(|at| at.elapsed() >= REFRESH) {
                    asked = Some(Instant::now());
                    let Ok(client) = this.read_with(cx, |t, _| t.client.clone()) else { break };
                    let res = call(client, "usage.stats", json!({ "days": 8 })).await;
                    let more = this.update(cx, |t, cx| {
                        match res {
                            Ok(v) => {
                                t.usage.stats = Some(Stats::decode(&v));
                                t.usage.supported = Some(true);
                            }
                            // A runner from before usage.stats.
                            Err(e) if e.code == -32601 => t.usage.supported = Some(false),
                            // Not connected just now: the next minute tries again.
                            Err(_) => {}
                        }
                        cx.notify();
                        t.usage.supported != Some(false)
                    });
                    if !more.unwrap_or(false) {
                        break;
                    }
                }
                cx.background_executor().timer(CHECK).await;
                let showing = this.read_with(cx, |t, cx| t.dock.read(cx).controller.is_showing(shell::USAGE)).unwrap_or(false);
                if !showing {
                    break;
                }
            }
            this.update(cx, |t, _| t.usage.poll = None).ok();
        }));
    }

    /// The chart's Today and 7 days: shown at once, and remembered.
    pub(crate) fn set_usage_range(&mut self, range: Range, cx: &mut Context<Self>) {
        if self.usage.range == range {
            return;
        }
        self.usage.range = range;
        self.usage.hovered = None;
        self.settings.saved.usage_chart_range = Some(range.name().to_owned());
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// The Settings card's switch: the meters go or come back at once, and
    /// it's remembered. Nothing goes to the runner.
    pub(crate) fn set_show_model_weeks(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.usage.show_model_weeks == on {
            return;
        }
        self.usage.show_model_weeks = on;
        self.settings.saved.usage_show_model_weeks = Some(on);
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// The login the next new thread would use (`newThreadAccountId`), and
    /// how full the pot is for it: this Mac's logins, or the other runner's a
    /// draft is set to start on.
    fn next_thread_room(&self) -> (f64, Option<String>) {
        let Some(logins) = self.new_thread_logins() else { return (0., None) };
        let id = self.with_draft(logins, |d| d.account_id());
        let level = rules::honey_level(logins.account(&id).and_then(rules::usage).as_ref());
        (level, Some(id))
    }

    /// The activity bar's Usage icon: the pot filled to the room left, and its
    /// tooltip, each pool's best room (Mike, 28 Oct).
    pub(crate) fn honey_pot(&mut self) -> (AnyElement, String) {
        let (level, login) = self.next_thread_room();
        self.usage.pot.see(level, login);
        let tip = rules::icon_tooltip(&rules::groups(&self.logins.pools, &self.logins.accounts), now_ms(), &Local);
        (self.usage.pot.element(), tip)
    }

    /// The panel in the dock: its own view, drawn again only when it has to
    /// be. A drag's ghost of it is drawn in place, as the Threads panel's is.
    pub(crate) fn usage_panel(&mut self, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if ghost {
            return self.render_usage_panel(true, window, cx);
        }
        let app = cx.entity();
        let view = self.usage.view.get_or_insert_with(|| cx.new(|cx| UsagePanelView::new(&app, cx))).clone();
        AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
    }

    /// The panel (`UsagePanel`).
    pub(crate) fn render_usage_panel(&mut self, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !ghost {
            self.keep_usage_fresh(cx);
        }
        let now = now_ms();
        let zone = Local;
        let p = palette(cx);
        let accounts = &self.logins.accounts;
        let stats = self.usage.stats.as_ref();
        // Until it's been drawn once, about the width it docks at.
        let width = Some(self.usage.width.get()).filter(|w| *w > 0.).unwrap_or(style::zoomed(320.));
        let content = (width - style::zoomed(32.)).max(0.);
        let mut column = v_flex().w_full().p(px(16.)).gap(px(14.));
        if rules::needs_newer_runner(accounts) {
            column = column.child(div().text_size(px(12.)).line_height(px(15.)).text_color(p.secondary).child(rules::NEWER_RUNNER));
        }
        if let Some(stats) = stats {
            let tiles = rules::tiles(stats, accounts, now, &zone);
            let chart = rules::chart(stats, accounts, self.usage.range, now, &zone);
            let measuring: &Window = window;
            let mut spend = v_flex()
                .gap(px(12.))
                .child(h_flex().items_start().gap(px(10.)).children(tiles.iter().enumerate().map(|(i, t)| stat_tile(i, t, content, &p, measuring))))
                .child(tokens_chart(&chart, accounts, self.usage.hovered, self.usage.plot.clone(), self.usage.range, now, &p, cx));
            if !stats.ready {
                spend = spend.child(div().text_size(px(11.)).line_height(px(14.)).text_color(p.tertiary).child(rules::PARTIAL));
            }
            column = column.child(spend);
        }
        let dark = cx.theme().is_dark();
        for card in rules::cards(&self.logins.pools, accounts, stats, self.usage.show_model_weeks, now, &zone) {
            column = column.child(pool_card(&card, accounts, content, dark, now, &p));
        }
        // Only when some login has a model's own week, or the switch would
        // do nothing.
        let model_weeks = rules::model_week_titles(accounts);
        if !model_weeks.is_empty() {
            column = column.child(settings_card(&model_weeks, self.usage.show_model_weeks, &p, cx));
        }
        if let Some(footer) = rules::footer(accounts, now, &zone) {
            column = column.child(div().text_size(px(11.)).line_height(px(14.)).text_color(p.tertiary).child(footer));
        }
        let scroll = self.usage.scroll.clone();
        let measured = self.usage.width.clone();
        div()
            .relative()
            .size_full()
            .child(div().id("usage-panel").size_full().overflow_y_scroll().track_scroll(&scroll).child(column))
            .child(div().absolute().inset_0().child(div().size_full().vertical_scrollbar(&scroll)))
            .when(!ghost, |s| {
                // The width for the next frame; a change draws it again at
                // once, so the tiles and meters settle in the same moment.
                s.child(
                    canvas(
                        move |bounds, window, _| {
                            let now = f32::from(bounds.size.width);
                            if (now - measured.get()).abs() > 0.5 {
                                measured.set(now);
                                window.request_animation_frame();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .into_any_element()
    }

    /// The panel as the automation's `dump` reports it, in the Swift app's
    /// shape (AppDump.Usage): whether it's open, its lines, and what each part
    /// says.
    #[cfg(feature = "automation")]
    pub(crate) fn usage_dump(&self, cx: &App) -> Value {
        let now = now_ms();
        let zone = Local;
        let accounts = &self.logins.accounts;
        let stats = self.usage.stats.as_ref();
        let (level, login) = self.next_thread_room();
        let mut notes = Vec::new();
        if rules::needs_newer_runner(accounts) {
            notes.push(rules::NEWER_RUNNER);
        }
        if stats.is_some_and(|s| !s.ready) {
            notes.push(rules::PARTIAL);
        }
        json!({
            "shown": self.dock.read(cx).controller.is_showing(shell::USAGE),
            "lines": rules::lines(&self.logins.pools, accounts, stats, now, &zone),
            "icon": {
                "percent": (level * 100.).round() as i64,
                "login": login,
                "tip": rules::icon_tooltip(&rules::groups(&self.logins.pools, accounts), now, &zone),
            },
            "tiles": stats.map(|s| rules::tiles(s, accounts, now, &zone).iter().map(|t| json!({ "title": t.title, "value": t.value, "detail": t.detail })).collect::<Vec<_>>()).unwrap_or_default(),
            "chart": stats.map(|s| {
                let c = rules::chart(s, accounts, self.usage.range, now, &zone);
                json!({ "title": c.title, "range": c.range.name(), "key": c.spending.iter().map(|id| rules::login_label(id, accounts)).collect::<Vec<_>>(), "empty": c.empty })
            }),
            "cards": rules::cards(&self.logins.pools, accounts, stats, self.usage.show_model_weeks, now, &zone).iter().map(|card| json!({
                "title": card.title,
                "today": card.today.as_ref().map(|t| &t.0),
                "logins": card.sections.iter().flat_map(|section| section.logins.iter().map(move |l| json!({
                    "name": l.name,
                    "provider": section.title,
                    "note": l.note,
                    "today": l.today.as_ref().map(|t| &t.0),
                    "status": l.status.map(LoginStatus::words),
                    "meters": l.meters.iter().map(|m| json!({ "title": m.title, "left": m.left, "verdict": m.verdict.as_ref().map(|v| &v.text) })).collect::<Vec<_>>(),
                    "more": l.more,
                }))).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "notes": notes,
            // The Settings card's switch: what it says and whether it's on,
            // while some login has a model's own week to switch.
            "settings": Some(rules::model_week_titles(accounts)).filter(|t| !t.is_empty()).map(|t| json!({ "switch": rules::model_weeks_switch(&t), "on": self.usage.show_model_weeks })),
            "footer": rules::footer(accounts, now, &zone),
        })
    }
}

/// `TokensChart`: tokens by the hour today, or by the day for the last week,
/// stacked by login. Hovering a bar shows what's in it.
#[allow(clippy::too_many_arguments)]
fn tokens_chart(chart: &Chart, accounts: &[Account], hovered: Option<i64>, plot: Rc<Cell<Option<Bounds<Pixels>>>>, range: Range, now: i64, p: &Palette, cx: &mut Context<Threadbear>) -> AnyElement {
    let accounts = accounts.to_vec();
    let dark = cx.theme().is_dark();
    let hovered = hovered.filter(|h| chart.slots.iter().any(|(s, _)| s == h));
    let segments = h_flex()
        .id("usage-range")
        .flex_none()
        .h(px(20.))
        .rounded(px(5.))
        .bg(p.track)
        .test_support()
        .aria_label("Show today by the hour, or the last 7 days by the day".to_owned())
        .tip("Show today by the hour, or the last 7 days by the day")
        .children([(Range::Today, "Today"), (Range::Week, "7 days")].map(|(r, title)| {
            let on = r == range;
            div()
                .id(SharedString::from(format!("usage-range-{}", r.name())))
                .w(px(53.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .when(on, |s| s.bg(p.selected_segment))
                .text_size(px(11.))
                .text_color(p.fg)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_usage_range(r, cx)))
                .child(title)
        }));
    let header = h_flex().items_center().child(div().text_size(px(12.5)).line_height(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(p.fg).child(chart.title)).child(div().flex_1()).child(segments);
    let drawn = chart.clone();
    let names = accounts.clone();
    let (grid, rule, labels) = (p.grid, p.now_rule, p.secondary);
    let paint_plot = plot.clone();
    let picture = canvas(|_, _, _| (), move |bounds, _, window, cx| paint_tokens(&drawn, &names, hovered, dark, grid, rule, labels, &paint_plot, bounds, window, cx)).size_full();
    let mut area = div()
        .id("usage-chart")
        .relative()
        .w_full()
        .h(px(130.))
        .child(picture)
        .on_mouse_move(cx.listener({
            let chart = chart.clone();
            move |this, e: &MouseMoveEvent, _, cx| {
                let slot = plot.get().filter(|b| b.contains(&e.position)).and_then(|b| {
                    let along = f32::from(e.position.x - b.origin.x) / f32::from(b.size.width).max(1.);
                    chart.slot_at(chart.start + ((chart.end - chart.start) as f64 * f64::from(along)) as i64)
                });
                if this.usage.hovered != slot {
                    this.usage.hovered = slot;
                    cx.notify();
                }
            }
        }))
        .on_hover(cx.listener(|this, over: &bool, _, cx| {
            if !*over && this.usage.hovered.take().is_some() {
                cx.notify();
            }
        }));
    if let Some(slot) = hovered {
        area = area.child(hover_note(chart, slot, &accounts, dark, now, p, cx));
    }
    if let Some(empty) = chart.empty {
        area = area.child(div().absolute().inset_0().flex().items_center().justify_center().text_size(px(11.)).text_color(p.tertiary).child(empty));
    }
    // The key: only the logins that spent something here, in the logins'
    // own order, so the colours read without a hover. Wrapped a whole
    // name at a time, as the Swift one is.
    let key = (chart.spending.len() > 1).then(|| {
        div().flex().flex_wrap().gap_x(px(12.)).gap_y(px(4.)).children(chart.spending.iter().map(|id| {
            h_flex()
                .flex_none()
                .gap(px(5.))
                .child(dot(login_colour(id, &accounts, dark), p))
                .child(div().text_size(px(11.)).line_height(px(14.)).text_color(p.secondary).whitespace_nowrap().child(rules::login_label(id, &accounts)))
        }))
    });
    card_box(v_flex().gap(px(8.)).p(px(12.)), p).child(header).child(area).children(key).into_any_element()
}

/// The colours the panel draws in, for this look.
struct Palette {
    fg: Hsla,
    secondary: Hsla,
    tertiary: Hsla,
    card: Hsla,
    card_edge: Hsla,
    divider: Hsla,
    grid: Hsla,
    now_rule: Hsla,
    track: Hsla,
    selected_segment: Hsla,
    bar_track: Hsla,
    bar_fill: Hsla,
    plot: Hsla,
    red: Hsla,
    orange: Hsla,
    green: Hsla,
    note: Hsla,
    note_edge: Hsla,
}

fn palette(cx: &App) -> Palette {
    let dark = cx.theme().is_dark();
    let system = |dark_hex: u32, light_hex: u32| -> Hsla { rgb(if dark { dark_hex } else { light_hex }).into() };
    let primary = |alpha: f32| style::primary_opacity(cx, alpha);
    Palette {
        fg: style::label_text(cx),
        secondary: style::secondary_text(cx),
        tertiary: style::tertiary_label(cx),
        // The cards' fill: a shade lighter than the panel, with a hairline.
        card: primary(0.035),
        card_edge: primary(0.07),
        divider: style::hairline(cx).opacity(0.6),
        grid: primary(0.06),
        now_rule: primary(0.25),
        // The range's segmented control, as macOS draws a small one
        // (measured from the Swift app's: the same in both looks).
        track: primary(0.075),
        selected_segment: primary(0.19),
        bar_track: primary(0.08),
        bar_fill: primary(0.45),
        plot: primary(0.025),
        // Color.red, .orange and .green: the system's.
        red: system(0xFF453A, 0xFF3B30),
        orange: system(0xFF9F0A, 0xFF9500),
        green: system(0x30D158, 0x34C759),
        note: cx.theme().popover,
        note_edge: style::hairline(cx),
    }
}

/// A login's colour (`LoginColours`), grey past the palette's eight.
fn login_colour(id: &str, accounts: &[Account], dark: bool) -> Hsla {
    match rules::login_colour(id, accounts, dark) {
        Some(hex) => rgb(hex).into(),
        None => hsla(0., 0., 0.56, 1.),
    }
}

/// `Card`: the fill and hairline behind the tiles, the chart and each pool.
/// The hairline is drawn over the edge rather than taking room inside it, as
/// SwiftUI's strokeBorder in a background is; GPUI's own border pushed every
/// card's insides 1 in and made each 2 taller.
fn card_box(el: Div, p: &Palette) -> Div {
    el.relative().rounded(px(8.)).bg(p.card).child(div().absolute().inset_0().rounded(px(8.)).border_1().border_color(p.card_edge))
}

/// `LoginDot`: a login's colour beside its name.
fn dot(colour: Hsla, _p: &Palette) -> Div {
    div().flex_none().size(px(7.)).rounded_full().bg(colour)
}

/// SwiftUI's `.monospacedDigit()`: the numbers don't shift as they change.
fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

fn text_width(text: &str, size: f32, weight: FontWeight, window: &Window) -> f32 {
    let font = Font { weight, ..window.text_style().font() };
    let run = TextRun { len: text.len(), font, color: Hsla::default(), background_color: None, underline: None, strikethrough: None };
    f32::from(window.text_system().shape_line(SharedString::from(text.to_owned()), gpui_kit::px(size), &[run], None).width)
}

/// `StatTile`: a number with its name over it and a quiet line under it, or
/// nothing there when the line doesn't fit (a narrow panel): the tip says it
/// either way. The number shrinks to fit, to six tenths of its size.
fn stat_tile(index: usize, tile: &Tile, content: f32, p: &Palette, window: &Window) -> AnyElement {
    let inner = ((content - style::zoomed(20.)) / 3. - style::zoomed(20.)).max(0.);
    let size = style::zoomed(22.);
    let natural = text_width(&tile.value, size, FontWeight::SEMIBOLD, window);
    let value_size = if natural > inner && natural > 0. { (inner / natural).max(0.6) * 22. } else { 22. };
    let detail = tile.detail.clone().filter(|d| text_width(d, style::zoomed(11.), FontWeight::NORMAL, window) <= inner);
    crate::highlights::tipped(card_box(div(), p).id(("usage-tile", index)).flex_1().min_w_0().p(px(10.)), tile.tip.clone())
        .child(
            v_flex()
                .gap(px(2.))
                .child(div().text_size(px(11.)).line_height(px(14.)).font_weight(FontWeight::MEDIUM).text_color(p.secondary).child(tile.title))
                .child(div().text_size(px(value_size)).line_height(px(26.)).font_weight(FontWeight::SEMIBOLD).font_features(tabular()).text_color(p.fg).whitespace_nowrap().child(tile.value.clone()))
                .child(div().text_size(px(11.)).line_height(px(14.)).text_color(p.tertiary).whitespace_nowrap().child(detail.unwrap_or_else(|| " ".to_owned()))),
        )
        .into_any_element()
}

/// The chart itself: grid lines, the bars stacked by login, the dashed line
/// at now on today's, and the axes' words. The pointer's slot stays as it is
/// and the rest fade, as the Swift chart's selection does.
#[allow(clippy::too_many_arguments)]
fn paint_tokens(chart: &Chart, accounts: &[Account], hovered: Option<i64>, dark: bool, grid: Hsla, rule: Hsla, labels: Hsla, plot_out: &Cell<Option<Bounds<Pixels>>>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let z = style::zoomed;
    let label_size = z(11.);
    let value_words: Vec<String> = chart.value_ticks.iter().map(|v| rules::tokens(*v)).collect();
    let widest = value_words.iter().map(|w| text_width(w, label_size, FontWeight::NORMAL, window)).fold(0., f32::max);
    // The value axis on the trailing side, its words 5 from the plot; the
    // time axis's words under it, 18 in all.
    let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let width = (f32::from(bounds.size.width) - widest - z(5.)).max(1.);
    let height = (f32::from(bounds.size.height) - z(18.)).max(1.);
    let plot = Bounds::new(point(gpui_kit::px(left), gpui_kit::px(top)), size(gpui_kit::px(width), gpui_kit::px(height)));
    plot_out.set(Some(plot));
    if !shown(bounds, window) {
        return;
    }
    let span = (chart.end - chart.start).max(1) as f32;
    let x_of = |at: i64| left + width * (at - chart.start) as f32 / span;
    let top_value = chart.value_ticks.last().copied().unwrap_or(0).max(1) as f32;
    let y_of = |v: f32| top + height - height * (v / top_value);
    // The time axis's lines are dashed, its words after each mark; the value
    // axis's lines solid, its words centred on them (Swift Charts' own look).
    for (at, _) in &chart.ticks {
        hairline(point(x_of(*at), top), point(x_of(*at), top + height), true, grid, window);
    }
    hairline(point(left + width, top), point(left + width, top + height), true, grid, window);
    for value in &chart.value_ticks {
        let y = y_of(*value as f32);
        hairline(point(left, y), point(left + width, y), false, grid, window);
    }
    let mut stacked: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for bar in &chart.bars {
        let Some(&(slot, length)) = chart.slots.iter().find(|(s, _)| *s == bar.start) else { continue };
        let below = *stacked.get(&slot).unwrap_or(&0);
        stacked.insert(slot, below.saturating_add(bar.tokens));
        let (x0, x1) = (x_of(slot), x_of(slot + length));
        // Four fifths of the slot, in its middle.
        let inset = (x1 - x0) * 0.1;
        let (y_low, y_high) = (y_of(below as f32), y_of((below + bar.tokens) as f32));
        let colour = login_colour(&bar.account_id, accounts, dark);
        let colour = if hovered.is_none_or(|h| h == slot) { colour } else { colour.opacity(0.45) };
        window.paint_quad(gpui_kit::fill(Bounds::new(point(gpui_kit::px(x0 + inset), gpui_kit::px(y_high)), size(gpui_kit::px((x1 - x0 - 2. * inset).max(0.5)), gpui_kit::px((y_low - y_high).max(0.)))), colour));
    }
    if let Some(at) = chart.now.filter(|at| *at >= chart.start && *at <= chart.end) {
        dashed_line(point(x_of(at), top), point(x_of(at), top + height), 1., rule, window);
    }
    for (value, words) in chart.value_ticks.iter().zip(&value_words) {
        paint_words(words, label_size, FontWeight::NORMAL, labels, point(left + width + z(5.), y_of(*value as f32)), Align::Left, window, cx);
    }
    // Each word after its mark, under the plot, unless it would run past the
    // plot's end into the value axis or into the word before (a narrow
    // panel), as Swift Charts leaves them out.
    let mut clear = left;
    for (at, words) in &chart.ticks {
        let x = x_of(*at) + z(3.5);
        let end = x + text_width(words, label_size, FontWeight::NORMAL, window);
        if x < clear || end > left + width + z(2.) {
            continue;
        }
        clear = end + z(4.);
        paint_words(words, label_size, FontWeight::NORMAL, labels, point(x, top + height + z(10.5)), Align::Left, window, cx);
    }
}

/// Whether a chart is in sight: one scrolled out of the panel isn't painted.
/// Its paths and dashes are most of a frame's work in a long panel, and
/// scrolling draws the panel again every frame.
fn shown(bounds: Bounds<Pixels>, window: &Window) -> bool {
    bounds.intersects(&window.content_mask().bounds)
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Centre,
}

/// One line of words, its middle at `at.y`.
#[allow(clippy::too_many_arguments)]
fn paint_words(text: &str, size: f32, weight: FontWeight, colour: Hsla, at: Point<f32>, align: Align, window: &mut Window, cx: &mut App) {
    let font = Font { weight, ..window.text_style().font() };
    let run = TextRun { len: text.len(), font, color: colour, background_color: None, underline: None, strikethrough: None };
    let line = window.text_system().shape_line(SharedString::from(text.to_owned()), gpui_kit::px(size), &[run], None);
    let line_height = (size * 1.3).ceil();
    let x = match align {
        Align::Left => at.x,
        Align::Centre => at.x - f32::from(line.width()) / 2.,
    };
    let _ = line.paint(point(gpui_kit::px(x), gpui_kit::px(at.y - line_height / 2.)), gpui_kit::px(line_height), TextAlign::Left, None, window, cx);
}

/// Swift Charts' grid line: half a point, dashed 3 on and 3 off down the time
/// axis. GPUI draws a path that thin at whole points as nothing at all, so
/// it's whole device pixels, on the pixel grid, the colour thinned to suit:
/// on a 1x screen one pixel at half the colour, as AppKit draws it there.
fn hairline(from: Point<f32>, to: Point<f32>, dashed: bool, colour: Hsla, window: &mut Window) {
    let scale = window.scale_factor().max(1.);
    let pixel = 1. / scale;
    let colour = colour.opacity((0.5 * scale).min(1.));
    let snap = |v: f32| (v * scale).floor() / scale;
    let step = style::zoomed(3.);
    let vertical = (from.x - to.x).abs() < 0.01;
    let (start, end) = if vertical { (from.y.min(to.y), from.y.max(to.y)) } else { (from.x.min(to.x), from.x.max(to.x)) };
    let mut at = start;
    while at < end {
        let until = if dashed { (at + step).min(end) } else { end };
        let piece = if vertical {
            Bounds::new(point(gpui_kit::px(snap(from.x)), gpui_kit::px(at)), size(gpui_kit::px(pixel), gpui_kit::px(until - at)))
        } else {
            Bounds::new(point(gpui_kit::px(at), gpui_kit::px(snap(from.y))), size(gpui_kit::px(until - at), gpui_kit::px(pixel)))
        };
        window.paint_quad(gpui_kit::fill(piece, colour));
        at = if dashed { at + 2. * step } else { end };
    }
}

fn dashed_line(from: Point<f32>, to: Point<f32>, width: f32, colour: Hsla, window: &mut Window) {
    let dash = gpui_kit::px(style::zoomed(3.));
    let mut builder = PathBuilder::stroke(gpui_kit::px(width)).dash_array(&[dash, dash]);
    builder.move_to(point(gpui_kit::px(from.x), gpui_kit::px(from.y)));
    builder.line_to(point(gpui_kit::px(to.x), gpui_kit::px(to.y)));
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

/// `HoverNote`: the hovered bar's hour or day, its total and each login's
/// part, over the chart's top corner.
fn hover_note(chart: &Chart, slot: i64, accounts: &[Account], dark: bool, now: i64, p: &Palette, _cx: &App) -> AnyElement {
    let mut bars: Vec<&rules::Bar> = chart.bars_at(slot);
    bars.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    let total = bars.iter().fold(0i64, |sum, b| sum.saturating_add(b.tokens));
    v_flex()
        .absolute()
        .top_0()
        .left_0()
        .gap(px(3.))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(p.note)
        .border_1()
        .border_color(p.note_edge)
        .shadow_sm()
        .child(div().text_size(px(11.)).line_height(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(p.fg).whitespace_nowrap().child(format!("{}  {}", chart.slot_words(slot, now, &Local), rules::tokens(total))))
        .children(bars.into_iter().map(|bar| {
            h_flex()
                .gap(px(5.))
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(p.secondary)
                .child(dot(login_colour(&bar.account_id, accounts, dark), p))
                .child(div().whitespace_nowrap().child(rules::login_label(&bar.account_id, accounts)))
                .child(div().flex_1().min_w(px(8.)))
                .child(div().font_features(tabular()).whitespace_nowrap().child(rules::tokens(bar.tokens)))
        }))
        .into_any_element()
}

/// `PoolCard`: the pool's icon, name and what it's spent today, then each
/// login in it. A pool of more than one provider names each provider over
/// its logins, as a Claude and a Codex login have different windows.
fn pool_card(card: &Card, accounts: &[Account], content: f32, dark: bool, now: i64, p: &Palette) -> AnyElement {
    let mut column = card_box(v_flex(), p).child(
        h_flex()
            .gap(px(7.))
            .px(px(12.))
            .py(px(9.))
            .child(icon(&card.symbol, 14., p.secondary))
            .child(div().text_size(px(13.)).line_height(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(p.fg).truncate().child(card.title.clone()))
            .child(div().flex_1())
            .children(card.today.as_ref().map(|(words, tip)| today_words(SharedString::from(format!("usage-card-today-{}", card.id)), words, tip, p))),
    );
    for (s, section) in card.sections.iter().enumerate() {
        if let Some(title) = &section.title {
            column = column.child(div().px(px(12.)).pt(px(6.)).text_size(px(11.)).line_height(px(14.)).font_weight(FontWeight::MEDIUM).text_color(p.tertiary).child(title.clone()));
        }
        for login in &section.logins {
            column = column.child(div().w_full().h(gpui_kit::px(1.)).bg(p.divider)).child(login_row(&format!("{}-{s}", card.id), login, accounts, content, dark, now, p));
        }
    }
    column.into_any_element()
}

/// `UsageSettings`: the panel's own settings, under the cards. For now
/// whether to show the models' own weeks, for someone who never runs that
/// model; its header the same as a pool's, the switch at the right edge in
/// line with the numbers above.
fn settings_card(model_weeks: &[String], on: bool, p: &Palette, cx: &mut Context<Threadbear>) -> AnyElement {
    // SF Symbols' gear is a bigger glyph than this icon set's at the same
    // size: 16 here draws it as wide as the Swift one, and puts the title
    // where it is there.
    let header = h_flex()
        .gap(px(7.))
        .child(icon("gearshape", 16., p.secondary))
        .child(div().text_size(px(13.)).line_height(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(p.fg).child("Settings"));
    let switch = crate::highlights::tipped(div().id("usage-model-weeks-holder").flex_none(), rules::MODEL_WEEKS_TIP).child(
        Switch::new("usage-model-weeks").checked(on).small().on_click(cx.listener(|this, on: &bool, _, cx| this.set_show_model_weeks(*on, cx))),
    );
    // As tall as macOS's small switch (20), which sets the row's height in
    // the Swift card; this app's small switch is 16.
    let row = h_flex()
        .items_center()
        .min_h(px(20.))
        .gap(px(8.))
        .child(div().min_w_0().text_size(px(12.)).line_height(px(15.)).text_color(p.fg).child(rules::model_weeks_switch(model_weeks)))
        .child(div().flex_1())
        .child(switch);
    card_box(v_flex().gap(px(8.)).px(px(12.)).py(px(10.)), p).child(header).child(row).into_any_element()
}

fn icon(symbol: &str, size: f32, colour: Hsla) -> Icon {
    icons::try_symbol(symbol).unwrap_or_else(|| icons::symbol("tray")).with_size(px(size)).text_color(colour)
}

/// "86M today", with its breakdown in the tip.
fn today_words(id: SharedString, words: &str, tip: &str, p: &Palette) -> AnyElement {
    crate::highlights::tipped(div().id(id).flex_none().text_size(px(11.)).line_height(px(14.)).font_features(tabular()).text_color(p.secondary).whitespace_nowrap(), tip.to_owned()).child(words.to_owned()).into_any_element()
}

/// `LoginRow`: its colour's dot, its name and note and what it's spent
/// today, its windows' meters side by side (two a row, or one over the other,
/// in a narrower panel), then only the extras that decide something.
fn login_row(key: &str, login: &LoginRow, accounts: &[Account], content: f32, dark: bool, now: i64, p: &Palette) -> AnyElement {
    let colour = login_colour(&login.id, accounts, dark);
    let id = |part: &str| SharedString::from(format!("usage-{key}-{}-{part}", login.id));
    let name_row = crate::highlights::tipped(h_flex().id(id("name")).gap(px(7.)).items_center(), login.tip.clone())
        .when(login.dot, |s| s.child(dot(colour, p)))
        .child(div().min_w_0().text_size(px(12.5)).line_height(px(15.)).font_weight(FontWeight::MEDIUM).text_color(p.fg).truncate().child(login.name.clone()))
        .children(login.note.as_ref().map(|note| div().min_w_0().text_size(px(11.)).line_height(px(14.)).text_color(p.tertiary).truncate().child(note.clone())))
        .child(div().flex_1().min_w(px(8.)))
        .children(login.today.as_ref().map(|(words, tip)| today_words(id("today"), words, tip, p)));
    let status = login.status.map(|s| {
        let limit = s == LoginStatus::LimitReached;
        div().text_size(px(11.)).line_height(px(14.)).when(limit, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if limit { p.red } else { p.tertiary }).child(s.words())
    });
    let meters = (!login.meters.is_empty()).then(|| meters_layout(&id(""), &login.meters, colour, content - style::zoomed(24.), now, p));
    let more = (!login.more.is_empty()).then(|| v_flex().gap(px(2.)).text_size(px(11.)).line_height(px(14.)).text_color(p.tertiary).children(login.more.iter().map(|line| div().child(line.clone()))));
    v_flex().w_full().gap(px(8.)).px(px(12.)).py(px(10.)).child(name_row).children(status).children(meters).children(more).into_any_element()
}

/// SwiftUI's ViewThatFits over the meters: a row of them when they fit at
/// their least (170 each, 18 apart), two a row for three in a panel too
/// narrow for that rather than three tall ones stacked, else one over the
/// other. Each takes its share of the row: capped, they stopped two thirds of
/// the way across a wide panel (Mike, 5 Nov).
fn meters_layout(key: &str, meters: &[Meter], colour: Hsla, room: f32, now: i64, p: &Palette) -> AnyElement {
    let z = style::zoomed;
    let fits = |n: usize| n as f32 * z(170.) + (n.saturating_sub(1)) as f32 * z(18.) <= room + 0.5;
    let meter = |i: usize| window_meter(&format!("{key}{i}"), &meters[i], colour, now, p);
    if fits(meters.len()) {
        return h_flex().items_start().gap(px(18.)).children((0..meters.len()).map(meter)).into_any_element();
    }
    if meters.len() > 2 && fits(2) {
        return v_flex()
            .gap(px(10.))
            .children((0..meters.len()).step_by(2).map(|i| {
                let second = (i + 1 < meters.len()).then(|| meter(i + 1)).unwrap_or_else(|| div().flex_1().min_w(px(170.)).into_any_element());
                h_flex().items_start().gap(px(18.)).child(meter(i)).child(second)
            }))
            .into_any_element();
    }
    v_flex().gap(px(10.)).children((0..meters.len()).map(meter)).into_any_element()
}

/// `WindowMeter`: its name and what's left, a bar (the 5-hour) or a chart of
/// the week, and what to do about it. Colour only where there's something to
/// do: red used up, orange runs out early, green room going spare.
fn window_meter(key: &str, meter: &Meter, colour: Hsla, now: i64, p: &Palette) -> AnyElement {
    let fill = if meter.remaining < 1. {
        p.red
    } else if meter.short {
        p.orange
    } else {
        p.bar_fill
    };
    let header = h_flex()
        .items_center()
        .child(div().text_size(px(11.)).line_height(px(15.)).font_weight(FontWeight::MEDIUM).text_color(p.secondary).truncate().child(meter.title.clone()))
        .child(div().flex_1().min_w(px(6.)))
        .child(div().flex_none().text_size(px(12.)).line_height(px(15.)).font_weight(FontWeight::MEDIUM).font_features(tabular()).text_color(if meter.short { fill } else { p.fg }).child(meter.left.clone()));
    // The weekly chart already shows what's left (its line ends at it), so
    // only the 5-hour window gets a bar. A model's own week is charted too.
    let picture: AnyElement = if meter.is_week() {
        let chart = meter.week_chart(now, &Local);
        let (grid, plot, labels, orange) = (p.grid, p.plot, p.secondary, p.orange);
        canvas(|_, _, _| (), move |bounds, _, window, cx| paint_week(&chart, colour, grid, plot, labels, orange, bounds, window, cx)).w_full().h(px(58.)).into_any_element()
    } else {
        let share = (meter.remaining / 100.).clamp(0., 1.) as f32;
        div().relative().w_full().h(px(4.)).rounded_full().bg(p.bar_track).child(div().absolute().top_0().left_0().h_full().w(relative(share)).rounded_full().bg(fill)).into_any_element()
    };
    let verdict = meter.verdict.as_ref().map(|v| {
        let (tone, loud) = match v.tone {
            Tone::Out => (p.red, true),
            Tone::Short => (p.orange, true),
            Tone::Spare => (p.green, false),
            Tone::Fine => (p.tertiary, false),
        };
        div().text_size(px(11.)).line_height(px(14.)).when(loud, |d| d.font_weight(FontWeight::MEDIUM)).text_color(tone).line_clamp(2).child(v.text.clone())
    });
    crate::highlights::tipped(v_flex().id(SharedString::from(format!("usage-meter-{key}"))).flex_1().min_w(px(170.)).gap(px(4.)), meter.tip.clone()).child(header).child(picture).children(verdict).into_any_element()
}

/// `WeekChart`: what was left over the last seven days, last week's readings
/// too, breaking at each reset, then this pace dashed on from now to the
/// reset, a faint line at each midnight and the days along the bottom.
#[allow(clippy::too_many_arguments)]
fn paint_week(chart: &rules::WeekChart, colour: Hsla, grid: Hsla, plot_fill: Hsla, labels: Hsla, orange: Hsla, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    if !shown(bounds, window) {
        return;
    }
    let z = style::zoomed;
    let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let width = f32::from(bounds.size.width).max(1.);
    // The plot, then 16 for the days' names.
    let height = (f32::from(bounds.size.height) - z(16.)).max(1.);
    window.paint_quad(gpui_kit::fill(Bounds::new(point(gpui_kit::px(left), gpui_kit::px(top)), size(gpui_kit::px(width), gpui_kit::px(height))), plot_fill));
    let span = (chart.to - chart.from).max(1) as f32;
    let x_of = |at: i64| left + width * (at - chart.from) as f32 / span;
    // A little room top and bottom, so the dots at full and empty aren't cut
    // in half by the edge.
    let pad = z(3.);
    let y_of = |left_percent: f64| top + pad + (height - 2. * pad) * (1. - (left_percent / 100.) as f32);
    for midnight in &chart.midnights {
        hairline(point(x_of(*midnight), top), point(x_of(*midnight), top + height), true, grid, window);
    }
    for run in &chart.runs {
        let points: Vec<Point<f32>> = run.iter().map(|s| point(x_of(s.at), y_of(s.remaining))).collect();
        if points.is_empty() {
            continue;
        }
        let floor = y_of(0.);
        let mut area = PathBuilder::fill();
        area.move_to(point(gpui_kit::px(points[0].x), gpui_kit::px(floor)));
        area.line_to(px_point(points[0]));
        monotone(&mut area, &points);
        area.line_to(point(gpui_kit::px(points[points.len() - 1].x), gpui_kit::px(floor)));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, colour.opacity(0.07));
        }
        if points.len() > 1 {
            let mut line = PathBuilder::stroke(gpui_kit::px(z(1.5)));
            line.move_to(px_point(points[0]));
            monotone(&mut line, &points);
            if let Ok(path) = line.build() {
                window.paint_path(path, colour);
            }
        }
    }
    if let [from, to] = chart.ahead.as_slice() {
        dashed_line(point(x_of(from.at), y_of(from.remaining)), point(x_of(to.at), y_of(to.remaining)), z(1.), if chart.short { orange.opacity(0.8) } else { colour.opacity(0.6) }, window);
    }
    // Swift Charts' symbolSize 18 is the dot's area.
    let radius = z((18f32 / std::f32::consts::PI).sqrt());
    paint_dot(point(x_of(chart.now.at), y_of(chart.now.remaining)), radius, colour, window);
    if let Some(out) = chart.runs_out {
        paint_dot(point(x_of(out), y_of(0.)), radius, orange, window);
    }
    // Each day's name under the middle of its day, or left out where it
    // would run off the chart or into the one before, as Swift Charts'
    // greedy collision resolution does (a reset just after noon left Sat's
    // name hanging off the end).
    let mut clear = left;
    for (noon, day) in &chart.noons {
        let half = text_width(day, z(9.5), FontWeight::NORMAL, window) / 2.;
        let x = x_of(*noon);
        if x - half < clear || x + half > left + width {
            continue;
        }
        clear = x + half + z(2.);
        paint_words(day, z(9.5), FontWeight::NORMAL, labels, point(x, top + height + z(9.)), Align::Centre, window, cx);
    }
}

fn px_point(p: Point<f32>) -> Point<Pixels> {
    point(gpui_kit::px(p.x), gpui_kit::px(p.y))
}

fn paint_dot(centre: Point<f32>, radius: f32, colour: Hsla, window: &mut Window) {
    let at = Bounds::new(point(gpui_kit::px(centre.x - radius), gpui_kit::px(centre.y - radius)), size(gpui_kit::px(radius * 2.), gpui_kit::px(radius * 2.)));
    window.paint_quad(gpui_kit::fill(at, colour).corner_radii(Corners::all(gpui_kit::px(radius))));
}

/// Swift Charts' `.monotone`: a smooth line through the points that never
/// overshoots them (Fritsch and Carlson's tangents), as Bézier pieces from
/// the first point on.
fn monotone(builder: &mut PathBuilder, points: &[Point<f32>]) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let slope = |i: usize| {
        let dx = points[i + 1].x - points[i].x;
        if dx.abs() < f32::EPSILON {
            0.
        } else {
            (points[i + 1].y - points[i].y) / dx
        }
    };
    let slopes: Vec<f32> = (0..n - 1).map(slope).collect();
    let mut tangents = vec![0f32; n];
    tangents[0] = slopes[0];
    tangents[n - 1] = slopes[n - 2];
    for i in 1..n - 1 {
        tangents[i] = if slopes[i - 1] * slopes[i] <= 0. { 0. } else { (slopes[i - 1] + slopes[i]) / 2. };
    }
    for i in 0..n - 1 {
        if slopes[i] == 0. {
            tangents[i] = 0.;
            tangents[i + 1] = 0.;
            continue;
        }
        let (a, b) = (tangents[i] / slopes[i], tangents[i + 1] / slopes[i]);
        let h = a * a + b * b;
        if h > 9. {
            let t = 3. / h.sqrt();
            tangents[i] = t * a * slopes[i];
            tangents[i + 1] = t * b * slopes[i];
        }
    }
    for i in 0..n - 1 {
        let (p0, p1) = (points[i], points[i + 1]);
        let third = (p1.x - p0.x) / 3.;
        let c1 = point(p0.x + third, p0.y + tangents[i] * third);
        let c2 = point(p1.x - third, p1.y - tangents[i + 1] * third);
        builder.cubic_bezier_to(px_point(p1), px_point(c1), px_point(c2));
    }
}

// ---- The honey pot ----

/// The pot's outline on the bear glyphs' 24 grid and line weight: body, rim,
/// lid and knob. Drawn as the other rail icons are, in the colour around it.
const POT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M8.2 9.6C4.2 10.6 3.6 19.4 7.6 20.8L16.4 20.8C20.4 19.4 19.8 10.6 15.8 9.6Z"/><rect x="7" y="7.1" width="10" height="2.5" rx="1.25"/><path d="M8.4 7.1C9 4.9 15 4.9 15.6 7.1"/><circle cx="12" cy="4.1" r="0.9"/></svg>"#;

/// The one drip, at its starting place on the shoulder of the pot.
const DRIP: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="black" d="M18.1 8.4C18.65 9.3 19.1 9.95 19.1 10.5A1 1 0 0 1 17.1 10.5C17.1 9.95 17.55 9.3 18.1 8.4Z"/></svg>"#;

/// The honey's colour: the only amber on the rail, and faint, as usage is the
/// one place amber means something.
fn honey() -> Hsla {
    Rgba { r: 217. / 255., g: 164. / 255., b: 65. / 255., a: 1. }.into()
}

/// What the pot shows, and the change it's part way through.
#[derive(Default)]
struct Pot {
    seen: bool,
    /// What it's filling to, 0 to 1, and for which login: the pot changing
    /// hands isn't a refill.
    level: f64,
    login: Option<String>,
    /// Where the last change set off from, and when.
    from: f64,
    since: Option<Instant>,
    /// The last change was the same login's window resetting: it rises over
    /// 700 ms with a drip, where a spend drains in a short step.
    refill: bool,
    /// Counts the changes, so each gets an animation of its own.
    changes: u64,
}

impl Pot {
    fn duration(&self) -> Duration {
        Duration::from_millis(if self.refill { 700 } else { 260 })
    }

    fn easing(&self) -> impl Fn(f32) -> f32 + 'static {
        // SwiftUI's `.timingCurve(0.2, 0.8, 0.3, 1)` for the rise, and its
        // `.easeOut` for a spend.
        if self.refill {
            timing_curve(0.2, 0.8, 0.3, 1.)
        } else {
            timing_curve(0., 0., 0.58, 1.)
        }
    }

    /// What's on show now: part way through a change, the level it's got to.
    fn shown(&self) -> f64 {
        let Some(since) = self.since else { return self.level };
        let t = (since.elapsed().as_secs_f32() / self.duration().as_secs_f32()).min(1.);
        self.from + (self.level - self.from) * f64::from((self.easing())(t))
    }

    /// The level and login now. The first is just shown, as the Swift pot's
    /// `onAppear` does; after that each change animates from wherever the
    /// last one has got to.
    fn see(&mut self, level: f64, login: Option<String>) {
        if !self.seen {
            *self = Pot { seen: true, level, from: level, login, ..Pot::default() };
            return;
        }
        if (level - self.level).abs() < 1e-9 && login == self.login {
            return;
        }
        let from = self.shown();
        self.refill = rules::refilled(self.level, level, login == self.login);
        self.from = from;
        self.level = level;
        self.login = login;
        self.since = Some(Instant::now());
        self.changes += 1;
    }

    /// The pot: the honey under the outline, slid up and down behind the
    /// body, and the drip after a refill. GPUI's animations stand in for
    /// SwiftUI's own, and under Reduce Motion both show where they end: the
    /// level just changes and there's no drip.
    fn element(&self) -> AnyElement {
        let (from, to) = (self.from, self.level);
        let colour = honey();
        let fill = div().absolute().inset_0().with_animation(("usage-honey", self.changes), Animation::new(self.duration()).with_easing(self.easing()), move |el, t| {
            let level = from + (to - from) * f64::from(t);
            el.child(canvas(|_, _, _| (), move |bounds, _, window, _| paint_honey(bounds, level, colour, window)).size_full())
        });
        let mut pot = div().relative().flex_none().size(px(21.)).child(fill).child(Icon::default().data(POT).with_size(px(21.)));
        if self.refill {
            // Near the top of the rise, as the honey gets to the rim: in for
            // 140 ms, then down the outside and gone over 340.
            let z = style::zoomed;
            let drip = div().absolute().top_0().left_0().size(px(21.)).child(Icon::default().data(DRIP).with_size(px(21.)).text_color(colour.opacity(0.9))).with_animations(
                ("usage-drip", self.changes),
                vec![Animation::new(Duration::from_millis(520)), Animation::new(Duration::from_millis(140)).with_easing(quadratic), Animation::new(Duration::from_millis(340)).with_easing(|t| 1. - (1. - t) * (1. - t))],
                move |el, part, t| {
                    let (opacity, x, y) = match part {
                        0 => (0., 0., 0.),
                        1 => (t, 0.7 * t, t),
                        _ => (1. - t, 0.7 + 1.2 * t, 1. + 4. * t),
                    };
                    el.opacity(opacity).left(gpui_kit::px(z(x))).top(gpui_kit::px(z(y)))
                },
            );
            pot = pot.child(drip);
        }
        pot.into_any_element()
    }
}

/// A CSS-style timing curve, as SwiftUI's `.timingCurve` and `.easeOut` are:
/// the curve's y where its x is `t`.
fn timing_curve(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 + 'static {
    move |t: f32| {
        if t <= 0. {
            return 0.;
        }
        if t >= 1. {
            return 1.;
        }
        let along = |a: f32, b: f32, s: f32| {
            let u = 1. - s;
            3. * u * u * s * a + 3. * u * s * s * b + s * s * s
        };
        // x rises with s for control points inside the unit square.
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..24 {
            let mid = (low + high) / 2.;
            if along(x1, x2, mid) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        along(y1, y2, (low + high) / 2.)
    }
}

/// The inside of the pot, from rim to floor (9.6 to 20.8 on the grid), as
/// short straight pieces.
fn pot_body(scale: f32) -> Vec<(f32, f32)> {
    let cubic = |p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p1: (f32, f32), out: &mut Vec<(f32, f32)>| {
        for i in 1..=16 {
            let t = i as f32 / 16.;
            let u = 1. - t;
            let at = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3. * u * u * t * b + 3. * u * t * t * c + t * t * t * d;
            out.push((at(p0.0, c1.0, c2.0, p1.0), at(p0.1, c1.1, c2.1, p1.1)));
        }
    };
    let mut points = vec![(8.2, 9.6)];
    cubic((8.2, 9.6), (4.2, 10.6), (3.6, 19.4), (7.6, 20.8), &mut points);
    points.push((16.4, 20.8));
    cubic((16.4, 20.8), (20.4, 19.4), (19.8, 10.6), (15.8, 9.6), &mut points);
    points.into_iter().map(|(x, y)| (x * scale, y * scale)).collect()
}

/// The honey: the pot's inside below the level.
fn paint_honey(bounds: Bounds<Pixels>, level: f64, colour: Hsla, window: &mut Window) {
    if level <= 0. {
        return;
    }
    let scale = f32::from(bounds.size.width) / 24.;
    let cut = (9.6 + (1. - level.clamp(0., 1.) as f32) * 11.2) * scale;
    // The body clipped to the part under the cut (Sutherland and Hodgman,
    // against one edge).
    let body = pot_body(scale);
    let mut kept: Vec<(f32, f32)> = Vec::new();
    for i in 0..body.len() {
        let (a, b) = (body[i], body[(i + 1) % body.len()]);
        let (a_in, b_in) = (a.1 >= cut, b.1 >= cut);
        if a_in != b_in {
            let t = (cut - a.1) / (b.1 - a.1);
            kept.push((a.0 + (b.0 - a.0) * t, cut));
        }
        if b_in {
            kept.push(b);
        }
    }
    if kept.len() < 3 {
        return;
    }
    let origin = bounds.origin;
    let points: Vec<Point<Pixels>> = kept.into_iter().map(|(x, y)| point(origin.x + gpui_kit::px(x), origin.y + gpui_kit::px(y))).collect();
    let mut builder = PathBuilder::fill();
    builder.add_polygon(&points, true);
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour.opacity(0.55));
    }
}

// ---- The automation ----

/// The Swift app's `usage` command: `show` opens or closes the panel, as the
/// icon does; `range` ("today" or "week") is the chart's control and
/// `showModelWeeks` the Settings card's switch; with none of them, the panel
/// as text. The CLIs' buttons it also takes in the Swift app are the Updates
/// panel's.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    if let Some(show) = cmd["show"].as_bool() {
        app.update(cx, |t, cx| {
            if t.dock.read(cx).controller.is_showing(shell::USAGE) != show {
                t.toggle_under_threads(shell::USAGE, cx);
            }
        });
        let shown = app.read(cx).dock.read(cx).controller.is_showing(shell::USAGE);
        return match (show, shown) {
            (true, true) => Ok(json!("ok shown")),
            (true, false) => Err("the panel didn't open".into()),
            (false, _) => Ok(json!("ok closed")),
        };
    }
    if let Some(range) = cmd["range"].as_str() {
        let range = Range::from_name(range).ok_or("range is today or week")?;
        app.update(cx, |t, cx| t.set_usage_range(range, cx));
        return Ok(json!(format!("ok {}", range.name())));
    }
    if let Some(on) = cmd["showModelWeeks"].as_bool() {
        app.update(cx, |t, cx| t.set_show_model_weeks(on, cx));
        return Ok(json!(if on { "ok on" } else { "ok off" }));
    }
    if cmd.get("click").is_some() {
        return Err("the CLIs' buttons are in the Updates panel".into());
    }
    let lines = app.read(cx).usage_dump(cx)["lines"].clone();
    Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())))
}
