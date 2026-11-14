// The columns of buttons down the window's edges: the activity bar on the left
// (ActivityBar.swift) and the rail of the thread's panels on the right
// (PanelsView.swift's PanelRail). Each button is the Swift app's ActivityButton:
// a grey icon in a 32 pt square, a faint square behind it on hover, lit when it's
// what's on show, with a small count or a dot in its corner.
use crate::app::Threadbear;
use crate::shell::{self, SidebarMode};
use crate::style::px;
use crate::{icons, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;
use threadbear_core::juice;
use threadbear_core::shortcuts::ShortcutCommand;
use crate::tips::Tip as _;

/// `ActivityBar.width`.
pub(crate) const BAR_WIDTH: f32 = 44.;
pub(crate) const BUTTON: f32 = 32.;

/// What one button says and does, built fresh each frame.
pub(crate) struct RailButton {
    pub(crate) id: SharedString,
    pub(crate) symbol: &'static str,
    /// Drawn instead of `symbol`: a patch panel's own icon and origin badge.
    pub(crate) face: Option<AnyElement>,
    pub(crate) tip: String,
    pub(crate) badge: Option<usize>,
    pub(crate) dot: bool,
    pub(crate) quiet_dot: bool,
    pub(crate) active: bool,
    /// Its key, shown beside it while ⌘ is held (key_hints.rs).
    pub(crate) hint: Option<String>,
    /// One of the views (Threads, Scheduled, Projects, Librarian), whose lit
    /// square slides between them; and whether it's sliding this frame.
    pub(crate) view: bool,
    pub(crate) lit_sliding: bool,
}

impl RailButton {
    fn new(id: impl Into<SharedString>, symbol: &'static str, title: impl Into<String>, active: bool) -> Self {
        Self { id: id.into(), symbol, face: None, tip: title.into(), badge: None, dot: false, quiet_dot: false, active, hint: None, view: false, lit_sliding: false }
    }
}

thread_local! {
    /// When each button was last clicked, for its bounce. The window draws
    /// on one thread.
    static TAPS: RefCell<HashMap<SharedString, Instant>> = RefCell::new(HashMap::new());
    /// Whether each button's count and dot showed last time it was drawn,
    /// and when the one showing turned up, to pop it in.
    static SHOWN: RefCell<HashMap<String, (bool, Option<Instant>)>> = RefCell::new(HashMap::new());
    /// The view buttons' lit square: where each button was laid out this
    /// frame, the one lit, and the square on its way from the last one.
    static LIT: RefCell<Lit> = RefCell::new(Lit::default());
}

#[derive(Default)]
struct Lit {
    bounds: HashMap<SharedString, Bounds<Pixels>>,
    key: Option<SharedString>,
    slide: Option<(SharedString, Instant)>,
}

/// Says which view's button is lit, before they're drawn, and whether its
/// square is sliding there from the last one (`Juice.selection` over the
/// square matched across the bar; Mike picked it, 13 Nov). Not under Reduce
/// Motion, where it jumps.
fn lit(key: Option<SharedString>, reduce: bool) -> bool {
    LIT.with(|lit| {
        let mut lit = lit.borrow_mut();
        if lit.key != key {
            let from = lit.key.take().filter(|from| !reduce && key.is_some() && lit.bounds.contains_key(from));
            lit.slide = from.map(|from| (from, Instant::now()));
            lit.key = key;
        }
        if lit.slide.as_ref().is_some_and(|(_, at)| at.elapsed() >= juice::LIT_SLIDE) {
            lit.slide = None;
        }
        lit.slide.is_some()
    })
}

/// The lit square on its way, under the view buttons: their column's first
/// child, so it paints before them, once they've all been placed.
fn lit_marker(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            LIT.with(|lit| {
                let lit = lit.borrow();
                let (Some(to), Some((from, at))) = (lit.key.as_ref(), lit.slide.as_ref()) else { return };
                let (Some(a), Some(b)) = (lit.bounds.get(from), lit.bounds.get(to)) else { return };
                let t = juice::snappy(at.elapsed().as_secs_f32(), juice::LIT_RESPONSE);
                let mix = |x: Pixels, y: Pixels| x + (y - x) * t;
                let bounds = Bounds::new(point(mix(a.origin.x, b.origin.x), mix(a.origin.y, b.origin.y)), b.size);
                window.paint_quad(fill(bounds, color).corner_radii(px(style::RAIL_CORNER)));
                window.request_animation_frame();
            })
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The icon, pressing in and bouncing back after a click (`ActivityButton`'s
/// taps): to 0.82 in 0.08 s, then a bouncy spring home.
fn bouncing(id: &SharedString, icon: Icon) -> AnyElement {
    let Some(at) = TAPS.with(|t| t.borrow().get(id).copied()).filter(|at| at.elapsed() < juice::RAIL_BOUNCE) else { return icon.into_any_element() };
    let length = juice::RAIL_BOUNCE;
    icon.with_animation(SharedString::from(format!("rail-bounce-{id}-{at:?}")), Animation::new(length), move |icon, t| {
        let scale = juice::rail_bounce(t * length.as_secs_f32());
        icon.transform(Transformation::scale(size(scale, scale)))
    })
    .into_any_element()
}

/// When the count or dot at `key` turned up, while it's still popping in
/// (`Juice.popIn`): it pops only on turning up, not when the app first draws
/// it, nor when it changes number (that rolls).
fn turned_up(key: String, shown: bool) -> Option<Instant> {
    SHOWN.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(key).or_insert((shown, None));
        if shown && !entry.0 {
            entry.1 = Some(Instant::now());
        }
        entry.0 = shown;
        if !shown {
            entry.1 = None;
        }
        entry.1.filter(|at| at.elapsed() < juice::POP)
    })
}

/// `ActivityButton`: an icon, the hover square, a lit square for the view on
/// show, and a count or a dot in the corner.
fn rail_button(button: RailButton, on_click: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static, cx: &mut Context<Threadbear>) -> AnyElement {
    let (fg, secondary) = (cx.theme().foreground, style::secondary_text(cx));
    let hover_fill = style::sidebar_hover(cx);
    let active_fill = style::primary_opacity(cx, 0.08);
    let background = cx.theme().background;
    let separator = style::hairline(cx);
    let dot_color = if button.dot { style::accent(cx) } else { secondary };
    let color = if button.active { fg } else { secondary };
    let (tip_text, label): (SharedString, SharedString) = (button.tip.clone().into(), button.tip.into());
    let tip_for_tooltip = tip_text.clone();
    let badge_key = button.id.clone();
    let tapped = button.id.clone();
    let face = match button.face {
        Some(face) => face,
        None => bouncing(&button.id, icons::symbol(button.symbol).with_size(px(15.))),
    };
    // A view button notes where it is for the lit square to slide between.
    let placed = button.id.clone();
    let marker = canvas(move |bounds, _, _| LIT.with(|lit| lit.borrow_mut().bounds.insert(placed.clone(), bounds)), |_, _, _, _| {}).absolute().top_0().left_0().size_full();
    let mut el = div()
        .id(button.id)
        .relative()
        .flex_none()
        .size(px(BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(style::RAIL_CORNER))
        .text_color(color)
        // Not while its lit square slides here: `lit_marker` draws it then.
        .when(button.active && !button.lit_sliding, |s| s.bg(active_fill))
        .when(!button.active, |s| s.hover(move |h| h.bg(hover_fill).text_color(fg)))
        .test_support()
        .role(Role::Button).aria_toggled(if button.active { Toggled::True } else { Toggled::False }).aria_label(label)
        .tip(tip_for_tooltip.clone())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if !cx.reduce_motion() {
                TAPS.with(|t| t.borrow_mut().insert(tapped.clone(), Instant::now()));
            }
            on_click(this, window, cx)
        }))
        .when(button.view, |s| s.child(marker))
        .child(face);
    let reduce = cx.reduce_motion();
    let pop_length = if reduce { juice::POP_REDUCED } else { juice::POP };
    let badge_shown = button.badge.is_some_and(|b| b > 0);
    let badge_pop = turned_up(format!("badge-{badge_key}"), badge_shown);
    let dot_pop = turned_up(format!("dot-{badge_key}"), !badge_shown && (button.dot || button.quiet_dot));
    if let Some(badge) = button.badge.filter(|b| *b > 0) {
        // Grey, not accent: a count of what's there, not an alarm. It rolls
        // to a new number (row_motion.rs), as the Swift badge does.
        let text = crate::row_motion::rolling(&format!("rail-{}", badge_key), badge, 12., cx.reduce_motion(), |n| div().child(if n > 99 { "99+".to_string() } else { n.to_string() }).into_any_element());
        let pill = div()
            .absolute()
            .top(px(-2.))
            .right(px(-3.))
            .min_w(px(14.))
            .h(px(14.))
            .px(px(3.5))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(background)
            .border_1()
            .border_color(separator)
            .text_size(px(9.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(secondary)
            .child(text);
        el = match badge_pop {
            // Turning up: it grows from a fifth of its size about its middle,
            // drawn smaller rather than scaled (GPUI can't scale a box).
            Some(at) => el.child(pill.with_animation(SharedString::from(format!("rail-badge-pop-{badge_key}-{at:?}")), Animation::new(pop_length), move |pill, t| {
                let (scale, opacity) = juice::pop_in(pop_length.mul_f32(t), reduce);
                let grow = 14. * (1. - scale) / 2.;
                pill.top(px(-2. + grow)).right(px(-3. + grow)).min_w(px(14. * scale)).h(px(14. * scale)).px(px(3.5 * scale)).text_size(px(9. * scale)).opacity(opacity)
            })),
            None => el.child(pill),
        };
    } else if button.dot || button.quiet_dot {
        let dot = div().absolute().top(px(1.)).right(px(1.)).size(px(7.)).rounded_full().bg(dot_color).border_1().border_color(background);
        el = match dot_pop {
            Some(at) => el.child(dot.with_animation(SharedString::from(format!("rail-dot-pop-{badge_key}-{at:?}")), Animation::new(pop_length), move |dot, t| {
                let (scale, opacity) = juice::pop_in(pop_length.mul_f32(t), reduce);
                let grow = 7. * (1. - scale) / 2.;
                dot.top(px(1. + grow)).right(px(1. + grow)).size(px(7. * scale)).opacity(opacity)
            })),
            None => el.child(dot),
        };
    }
    // Its key to the right while ⌘ is held (`shortcutHint(_, .after)`).
    el.children(crate::key_hints::hint(button.hint, crate::key_hints::Placement::After)).into_any_element()
}

impl Threadbear {
    fn shortcut_tip(&self, title: &str, command: ShortcutCommand) -> String {
        match self.shortcuts.key(command) {
            Some(key) => format!("{title} ({})", key.display()),
            None => title.to_string(),
        }
    }

    /// A command's key as the menus show it ("⌘1"), if it has one.
    pub(crate) fn shortcut_display(&self, command: ShortcutCommand) -> Option<String> {
        self.shortcuts.key(command).map(|k| k.display())
    }

    /// The Inbox's thread count, for the Threads button's badge.
    fn inbox_count(&self, cx: &App) -> usize {
        if let Some(count) = self.inbox_count_cache.get() {
            return count;
        }
        let now = crate::now_ms();
        let count = self.shown_sections(cx, now).iter().find(|s| s.key == "inbox").map_or(0, |s| s.rows.len());
        self.inbox_count_cache.set(Some(count));
        count
    }

    /// `WindowGrip`: the strip under the traffic lights, which moves the
    /// window as the bars beside it do, double-click zooms.
    fn window_grip(&self) -> impl IntoElement {
        div().id("window-grip").w_full().flex_none().h(px(crate::header::HEADER_HEIGHT)).on_mouse_down(MouseButton::Left, |e: &MouseDownEvent, window, _| {
            if e.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
    }

    /// The column down the left (`ActivityBar`).
    pub(crate) fn render_activity_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inbox_count = self.inbox_count(cx);
        let mut views = vec![];
        let mode = self.mode;
        // The view on show; its button's square slides to it from the last.
        let lit_id: SharedString = match mode {
            SidebarMode::Inbox => "rail-threads",
            SidebarMode::Scheduled => "rail-scheduled",
            SidebarMode::Projects => "rail-projects",
            SidebarMode::Librarian => "rail-librarian",
        }
        .into();
        let sliding = lit(Some(lit_id.clone()), cx.reduce_motion());
        let view_button = |mut button: RailButton| {
            button.view = true;
            button.lit_sliding = sliding && button.id == lit_id;
            button
        };
        let mut button = view_button(RailButton::new("rail-threads", "bubble.left.and.bubble.right", "Threads", mode == SidebarMode::Inbox));
        button.tip = self.shortcut_tip("Threads", ShortcutCommand::ShowInbox);
        button.hint = self.shortcut_display(ShortcutCommand::ShowInbox);
        button.badge = Some(inbox_count);
        views.push(rail_button(button, |this, _, cx| {
            this.unfold_threads(cx);
            this.set_mode(SidebarMode::Inbox, cx)
        }, cx));
        if self.schedules_supported {
            let mut button = view_button(RailButton::new("rail-scheduled", "clock", "Scheduled", mode == SidebarMode::Scheduled));
            button.tip = self.shortcut_tip("Scheduled", ShortcutCommand::ShowScheduled);
            button.hint = self.shortcut_display(ShortcutCommand::ShowScheduled);
            button.badge = Some(self.scheduled.active_count());
            views.push(rail_button(button, |this, window, cx| {
                this.unfold_threads(cx);
                this.set_mode(SidebarMode::Scheduled, cx);
                // Its page takes the thread's place, and the list the
                // keyboard, so Esc there goes back to the Inbox.
                window.focus(&this.sidebar.focus, cx);
                crate::web_panels::keyboard_to_app(window);
            }, cx));
        }
        if self.projects_supported {
            let mut button = view_button(RailButton::new("rail-projects", "square.stack.3d.up", "Projects", mode == SidebarMode::Projects));
            button.tip = self.shortcut_tip("Projects", ShortcutCommand::ShowProjects);
            button.hint = self.shortcut_display(ShortcutCommand::ShowProjects);
            views.push(rail_button(button, |this, window, cx| {
                this.unfold_threads(cx);
                this.set_mode(SidebarMode::Projects, cx);
                // Its page takes the thread's place, and the list the
                // keyboard, so Esc there goes back to the Inbox.
                window.focus(&this.sidebar.focus, cx);
                crate::web_panels::keyboard_to_app(window);
            }, cx));
            let mut button = view_button(RailButton::new("rail-librarian", "books.vertical", "Librarian", mode == SidebarMode::Librarian));
            button.tip = self.shortcut_tip("Librarian", ShortcutCommand::ShowLibrarian);
            button.hint = self.shortcut_display(ShortcutCommand::ShowLibrarian);
            // The look under way and the ones waiting (`pendingCount`).
            button.badge = self.librarian.status.as_ref().map(|s| s.pending_count());
            views.push(rail_button(button, |this, window, cx| {
                this.unfold_threads(cx);
                this.set_mode(SidebarMode::Librarian, cx);
                // Its page takes the thread's place, and the list the
                // keyboard, so Esc there goes back to the Inbox.
                window.focus(&this.sidebar.focus, cx);
                crate::web_panels::keyboard_to_app(window);
            }, cx));
        }
        // Connectors, Runners and Updates at the foot (activity_panels.rs),
        // each with its dot: the accent when only Mike can do something, grey
        // while something sorts itself out.
        let mut foot = vec![];
        // Finish setting up, with how many are left: only while something's
        // left and it isn't hidden (setup_panel.rs).
        if self.setup_shows_icon() {
            let mut button = RailButton::new("rail-setup", "checklist", "Finish setting up", self.activity_panel_showing(shell::SETUP, cx));
            button.badge = Some(self.setup_checklist().left());
            foot.push(rail_button(button, |this, _, cx| this.toggle_under_threads(shell::SETUP, cx), cx));
        }
        if self.connectors_on() {
            let status = self.connectors_panel.status.as_ref();
            let mut button = RailButton::new("rail-connectors", "powerplug", threadbear_core::connectors::tooltip(status), self.activity_panel_showing(shell::CONNECTORS, cx));
            button.dot = threadbear_core::connectors::needs_you(status);
            button.quiet_dot = threadbear_core::connectors::quiet(status);
            foot.push(rail_button(button, |this, _, cx| this.toggle_under_threads(shell::CONNECTORS, cx), cx));
        }
        // Only with more than one runner: a panel of just this Mac says
        // nothing the app doesn't.
        if self.remote.all_runners.len() > 1 {
            let runners = &self.remote.all_runners;
            let mut button = RailButton::new("rail-runners", "server.rack", threadbear_core::runners::tooltip(runners), self.activity_panel_showing(shell::RUNNERS, cx));
            // Worth a look, but not Mike's to fix from here.
            button.quiet_dot = runners.iter().any(|r| threadbear_core::runners::state(r) != threadbear_core::runners::State::Online);
            foot.push(rail_button(button, |this, _, cx| this.toggle_under_threads(shell::RUNNERS, cx), cx));
        }
        let mut button = RailButton::new("rail-updates", "arrow.down.circle", self.updates_tooltip(), self.activity_panel_showing(shell::UPDATES, cx));
        button.dot = self.updates_dot();
        button.quiet_dot = self.updates_quiet_dot();
        let updates = rail_button(button, |this, _, cx| this.toggle_under_threads(shell::UPDATES, cx), cx);
        // Right-click: the app's update in a popover (updates_panel.rs).
        foot.push(self.with_update_popover(updates, window, cx));
        // Usage, as a honey pot filled to the room the next thread would get,
        // its tooltip each pool's best room (usage.rs). Toggles its panel
        // under the threads, as VS Code's bar does its views.
        let usage_shown = self.dock.read(cx).controller.is_showing(shell::USAGE);
        let (pot, tip) = self.honey_pot();
        let mut button = RailButton::new("rail-usage", "gauge.with.dots.needle.33percent", "Usage", usage_shown);
        button.face = Some(pot);
        button.tip = tip;
        let usage = rail_button(button, |this, _, cx| this.toggle_under_threads(shell::USAGE, cx), cx);
        let mut button = RailButton::new("rail-settings", "gearshape", "Settings", false);
        button.tip = self.shortcut_tip("Settings", ShortcutCommand::Settings);
        button.hint = self.shortcut_display(ShortcutCommand::Settings);
        let settings = rail_button(button, |this, window, cx| this.run_command(shell::Command::Settings, window, cx), cx);
        let hairline = cx.theme().border;
        div()
            .relative()
            .flex_none()
            .w(px(BAR_WIDTH))
            .h_full()
            // Starts under the title bar strip: the traffic lights are wider
            // than the bar, so a full-height line cut between the yellow and
            // green dots (ActivityBar).
            .child(div().absolute().top(px(crate::header::HEADER_HEIGHT)).bottom_0().right_0().w(gpui_kit::px(1.)).bg(hairline.opacity(0.7)))
            .child(
                v_flex()
                    .size_full()
                    .items_center()
                    .pb(px(10.))
                    .gap(px(4.))
                    .child(self.window_grip())
                    // The views together, for the tour to light up (not the
                    // icons at the foot), over the lit square on its way.
                    .child(v_flex().relative().items_center().gap(px(4.)).child(lit_marker(style::primary_opacity(cx, 0.08))).children(views).children(crate::tour::mark(threadbear_core::tour::Target::ActivityBar)))
                    .child(div().flex_1().min_h(px(8.)))
                    .children(foot)
                    .child(usage)
                    .child(settings),
            )
    }

    /// The column down the right (`PanelRail`): Highlights at the top, a
    /// button per panel, Files, and Patches at the foot. Only while a thread
    /// is open.
    pub(crate) fn render_right_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // On a project's page, the panels it borrows (`pane_thread`), with no
        // Highlights or Files: they're a thread's.
        let project_page = self.projects.open.is_some();
        let thread = self.pane_thread();
        let Some(thread) = thread else { return div().into_any_element() };
        let dock = self.dock.read(cx);
        let showing = |panel: &str| dock.controller.is_showing(panel);
        let highlights_shown = showing(shell::HIGHLIGHTS);
        let files_shown = showing(shell::FILES);
        let patches_shown = showing(shell::PATCHES);
        let browser_shown = showing(shell::BROWSER);
        // The accent dot while the last turn's asks aren't all ticked.
        let open_ask = thread.asks.iter().flatten().any(|a| !a.ticked.unwrap_or(false)) && thread.asks_seq.is_some();
        let panels: Vec<(threadbear_core::highlights::Panel, bool)> = thread.panels().into_iter().map(|p| {
            // Or the thread's pinned copy of it (pinned_panels.rs).
            let shown = dock.controller.is_showing(&p.id) || dock.controller.is_showing(&crate::pinned_panels::pinned_panel(&thread.id, &p.id));
            (p, shown)
        }).collect();
        let _ = dock;
        let mut column = vec![];
        let mut highlights = RailButton::new("rail-highlights", "pin", "Highlights", highlights_shown);
        highlights.dot = open_ask;
        if !project_page {
            let button = rail_button(highlights, |this, _, cx| this.click_rail(shell::HIGHLIGHTS, cx), cx);
            // The tour's Highlights step lights this when the panel's closed.
            column.push(div().relative().child(button).children(crate::tour::mark(threadbear_core::tour::Target::HighlightsRail)).into_any_element());
        }
        for (panel, shown) in panels {
            // Its own icon, with where it came from in the corner and in the
            // tip ("Convex, from the Threadbear project"), as the Swift rail.
            let mut b = RailButton::new(SharedString::from(format!("rail-panel-{}", panel.id)), "square.dashed", crate::patch_icons::panel_tip(&panel, &thread), shown);
            b.face = Some(crate::patch_icons::panel_face(&panel, &thread, &self.sidebar.project_icons, 15., 13., cx));
            b.badge = panel.badge.map(|n| n.max(0) as usize);
            let id = panel.id.clone();
            column.push(rail_button(b, move |this, _, cx| this.click_rail(&id, cx), cx));
        }
        // BrowserBear's pages for this thread, while it's on this Mac: their
        // count, and the accent dot while one's handed to Mike.
        if let Some((pages, yours)) = self.browser_rail(&thread.id, cx).filter(|_| !project_page) {
            let mut b = RailButton::new("rail-browser", "globe", "This thread's browser", browser_shown);
            b.badge = Some(pages);
            b.dot = yours;
            column.push(rail_button(b, |this, _, cx| this.click_rail(shell::BROWSER, cx), cx));
        }
        if !project_page {
            column.push(rail_button(RailButton::new("rail-files", "folder", "Files", files_shown), |this, _, cx| this.click_rail(shell::FILES, cx), cx));
        }
        let patches = thread.patches.as_ref().map_or(0, Vec::len);
        let hairline = cx.theme().border;
        let mut rail = v_flex().size_full().items_center().pt(px(4.)).pb(px(10.)).gap(px(4.)).children(column).child(div().flex_1().min_h(px(8.)));
        if patches > 0 {
            let mut b = RailButton::new("rail-patches", "square.dashed", "Patches", patches_shown);
            b.badge = Some(patches);
            rail = rail.child(rail_button(b, |this, _, cx| this.click_rail(shell::PATCHES, cx), cx));
        }
        div().relative().flex_none().w(px(BAR_WIDTH)).h_full().child(div().absolute().top(px(4.)).bottom(px(4.)).left_0().w(gpui_kit::px(1.)).bg(hairline.opacity(0.7))).child(rail).into_any_element()
    }

    /// The rails as `dump` reports them, with the fields the Swift app's has.
    #[cfg(feature = "automation")]
    pub(crate) fn rails_dump(&self, cx: &App) -> serde_json::Value {
        use serde_json::json;
        let mode = self.mode;
        let mut activity = vec![json!({ "title": "Threads", "badge": self.inbox_count(cx), "active": mode == SidebarMode::Inbox })];
        if self.schedules_supported {
            activity.push(json!({ "title": "Scheduled", "badge": self.scheduled.active_count(), "active": mode == SidebarMode::Scheduled }));
        }
        if self.projects_supported {
            activity.push(json!({ "title": "Projects", "active": mode == SidebarMode::Projects }));
            activity.push(json!({ "title": "Librarian", "badge": self.librarian.status.as_ref().map(|s| s.pending_count()), "active": mode == SidebarMode::Librarian }));
        }
        // The foot's panels, with their dots: `dot` the accent, `quietDot` the grey.
        if self.connectors_on() {
            let status = self.connectors_panel.status.as_ref();
            activity.push(json!({ "title": "Connectors", "dot": threadbear_core::connectors::needs_you(status), "quietDot": threadbear_core::connectors::quiet(status), "active": self.activity_panel_showing(shell::CONNECTORS, cx) }));
        }
        if self.remote.all_runners.len() > 1 {
            let quiet = self.remote.all_runners.iter().any(|r| threadbear_core::runners::state(r) != threadbear_core::runners::State::Online);
            activity.push(json!({ "title": "Runners", "dot": false, "quietDot": quiet, "active": self.activity_panel_showing(shell::RUNNERS, cx) }));
        }
        activity.push(json!({ "title": "Updates", "dot": self.updates_dot(), "quietDot": self.updates_quiet_dot(), "active": self.activity_panel_showing(shell::UPDATES, cx) }));
        let mut right = vec![];
        if let Some(thread) = self.pane_thread() {
            let project_page = self.projects.open.is_some();
            let dock = self.dock.read(cx);
            let showing = |p: &str| dock.controller.is_showing(p);
            let open_ask = thread.asks.iter().flatten().any(|a| !a.ticked.unwrap_or(false)) && thread.asks_seq.is_some();
            if !project_page {
                right.push(json!({ "title": "Highlights", "dot": open_ask, "active": showing(shell::HIGHLIGHTS) }));
            }
            for p in thread.panels.iter().flatten() {
                if let (Some(id), Some(title)) = (p["id"].as_str(), p["title"].as_str()) {
                    right.push(json!({ "title": title, "badge": p["badge"], "active": showing(id) }));
                }
            }
            if let Some((pages, yours)) = self.browser_rail(&thread.id, cx).filter(|_| !project_page) {
                right.push(json!({ "title": "Browser", "badge": pages, "dot": yours, "active": showing(shell::BROWSER) }));
            }
            if !project_page {
                right.push(json!({ "title": "Files", "active": showing(shell::FILES) }));
            }
            if thread.patches.as_ref().is_some_and(|p| !p.is_empty()) {
                right.push(json!({ "title": "Patches", "badge": thread.patches.as_ref().map(Vec::len), "active": showing(shell::PATCHES) }));
            }
        }
        json!({ "activity": activity, "right": right })
    }
}


