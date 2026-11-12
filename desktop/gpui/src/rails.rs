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
use gpui_kit::component::{v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
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
}

impl RailButton {
    fn new(id: impl Into<SharedString>, symbol: &'static str, title: impl Into<String>, active: bool) -> Self {
        Self { id: id.into(), symbol, face: None, tip: title.into(), badge: None, dot: false, quiet_dot: false, active, hint: None }
    }
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
        .when(button.active, |s| s.bg(active_fill))
        .when(!button.active, |s| s.hover(move |h| h.bg(hover_fill).text_color(fg)))
        .test_support()
        .aria_label(label)
        .tip(tip_for_tooltip.clone())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
        .child(match button.face {
            Some(face) => face,
            None => icons::symbol(button.symbol).with_size(px(15.)).into_any_element(),
        });
    if let Some(badge) = button.badge.filter(|b| *b > 0) {
        // Grey, not accent: a count of what's there, not an alarm. It rolls
        // to a new number (row_motion.rs), as the Swift badge does.
        let text = crate::row_motion::rolling(&format!("rail-{}", badge_key), badge, 12., cx.reduce_motion(), |n| div().child(if n > 99 { "99+".to_string() } else { n.to_string() }).into_any_element());
        el = el.child(
            div()
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
                .child(text),
        );
    } else if button.dot || button.quiet_dot {
        el = el.child(div().absolute().top(px(1.)).right(px(1.)).size(px(7.)).rounded_full().bg(dot_color).border_1().border_color(background));
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
        let mut button = RailButton::new("rail-threads", "bubble.left.and.bubble.right", "Threads", mode == SidebarMode::Inbox);
        button.tip = self.shortcut_tip("Threads", ShortcutCommand::ShowInbox);
        button.hint = self.shortcut_display(ShortcutCommand::ShowInbox);
        button.badge = Some(inbox_count);
        views.push(rail_button(button, |this, _, cx| {
            this.unfold_threads(cx);
            this.set_mode(SidebarMode::Inbox, cx)
        }, cx));
        if self.schedules_supported {
            let mut button = RailButton::new("rail-scheduled", "clock", "Scheduled", mode == SidebarMode::Scheduled);
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
            let mut button = RailButton::new("rail-projects", "square.stack.3d.up", "Projects", mode == SidebarMode::Projects);
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
            let mut button = RailButton::new("rail-librarian", "books.vertical", "Librarian", mode == SidebarMode::Librarian);
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
                    .child(v_flex().items_center().gap(px(4.)).children(views))
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
        // The accent dot while the last turn's asks aren't all ticked.
        let open_ask = thread.asks.iter().flatten().any(|a| !a.ticked.unwrap_or(false)) && thread.asks_seq.is_some();
        let panels: Vec<(threadbear_core::highlights::Panel, bool)> = thread.panels().into_iter().map(|p| {
            let shown = dock.controller.is_showing(&p.id);
            (p, shown)
        }).collect();
        let _ = dock;
        let mut column = vec![];
        let mut highlights = RailButton::new("rail-highlights", "pin", "Highlights", highlights_shown);
        highlights.dot = open_ask;
        if !project_page {
            column.push(rail_button(highlights, |this, _, cx| this.click_rail(shell::HIGHLIGHTS, cx), cx));
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


