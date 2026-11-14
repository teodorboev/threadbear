// Open Anything, ⌘K: a box centred over the window, the rest dimmed, that
// opens a thread, a panel, a project or one of the app's pages by name. What
// it lists is threadbear_core::open_anything; this draws it and does the
// opening. Mike picked the design ("A, the palette") in thread 5d33618f; it's
// the GPUI app's own, so the Swift app hasn't got it.
//
// Like the slash palette (slash_palette.rs) the cursor stays in the box and
// the arrows, Return and Esc reach the list from the box's own key handling,
// and the list scrolls to keep the pick in sight. It's a deferred layer on the
// main window rather than a window of its own, over the web panels' pages,
// which go out of sight while it's up (`web_panels::sweeper`).
//
// The box is a view of its own (`AnythingBox`) so a letter typed redraws
// only the box: when it was part of the app's own view, every letter told
// the whole app it had changed, and the threads list, the transcript and the
// side pane (which follow the app) drew again before the letter showed.
// The list it searches (`Catalog`) is made once as the box opens and again
// at most every 400 ms while the app changes under it, and each search runs
// on a background thread; the letter shows straight away and the matches a
// moment after (Mike, 13 Nov: "if I type lots of letters in rapid
// succession, there's a noticeable lag").
use crate::app::Threadbear;
use crate::shell::{self, Command};
use crate::style::{self, size};
use crate::tips::Tip as _;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{Enter, Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use threadbear_core::open_anything::{self as rules, Catalog, Group, Item, Kind, Page, PageEntry, Target};
use threadbear_core::shortcuts::ShortcutCommand;
use threadbear_dock::{DockDrop, DockEdge};

/// The box's width at 100%, and its top's distance from the window's top
/// as a share of the window's height (the mockup's 110 of 633).
const WIDTH: f32 = 580.;
const TOP_SHARE: f32 = 0.17;
const ROW_HEIGHT: f32 = 28.;
const CORNER: f32 = 12.;

/// The box while it's up, as the app keeps it.
pub(crate) struct OpenAnything {
    pub(crate) view: Entity<AnythingBox>,
    /// What had the keyboard before, given back when it closes with nothing
    /// opened (Esc, a click outside).
    before: Option<FocusHandle>,
}

/// How long the app has to have changed before the box's list is made again
/// from it: often enough that a thread started meanwhile shows, rarely
/// enough that a streaming reply doesn't keep it busy.
const REBUILD_EVERY: Duration = Duration::from_millis(400);

/// The box itself: what's typed, the list it searches and what matched.
pub(crate) struct AnythingBox {
    app: WeakEntity<Threadbear>,
    pub(crate) input: Entity<InputState>,
    /// The pick's key; none is the first entry.
    selected: Option<String>,
    scroll: ScrollHandle,
    catalog: Arc<Catalog>,
    /// Recent as it was when the box opened: opening anything closes it.
    recent: Arc<Vec<String>>,
    groups: Arc<Vec<Group>>,
    /// The words `groups` are for; the box may hold newer ones for a moment.
    matched: String,
    /// The search under way; a newer one drops it.
    searching: Option<Task<()>>,
    rebuilding: Option<Task<()>>,
    _typing: Subscription,
    _app_changed: Subscription,
}

impl AnythingBox {
    fn new(app: &Entity<Threadbear>, catalog: Catalog, recent: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(rules::PROMPT));
        let typing = cx.subscribe(&input, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.typed(cx);
            }
        });
        let app_changed = cx.observe(app, |this, _, cx| this.app_changed(cx));
        let groups = catalog.groups("", &recent);
        Self {
            app: app.downgrade(),
            input,
            selected: None,
            scroll: ScrollHandle::new(),
            catalog: Arc::new(catalog),
            recent: Arc::new(recent),
            groups: Arc::new(groups),
            matched: String::new(),
            searching: None,
            rebuilding: None,
            _typing: typing,
            _app_changed: app_changed,
        }
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// The words changed: the pick goes back to the first match (any edit
    /// does, so typing a letter and taking it out again before its search is
    /// back can't leave an older pick on), and the new words are searched.
    fn typed(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.scroll.scroll_to_item(0);
        self.search(cx);
        cx.notify();
    }

    /// The words changed: match them on a background thread, and show what
    /// matched when it's back, unless newer words have taken its place.
    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let (catalog, recent, words) = (self.catalog.clone(), self.recent.clone(), query.clone());
        let working = cx.background_executor().spawn(async move { catalog.groups(&words, &recent) });
        // Replacing the task drops the one before, and its search with it.
        self.searching = Some(cx.spawn(async move |this, cx| {
            let groups = working.await;
            let _ = this.update(cx, |this, cx| {
                this.searching = None;
                this.show(query, groups);
                // The pick can be a page named for the words further down
                // than the first group (`rules::picked`): keep it in sight.
                if this.selected.is_none() {
                    let first_group = this.groups.first().map_or(0, |g| g.items.len());
                    if let Some(at) = row_index(&this.groups, None).filter(|i| *i > first_group) {
                        this.scroll.scroll_to_item(at);
                    }
                }
                cx.notify();
            });
        }));
    }

    /// What matched, kept. The pick stays as it is: `typed` reset it when
    /// the words changed, and a list made again for the same words keeps it.
    fn show(&mut self, query: String, groups: Vec<Group>) {
        self.groups = Arc::new(groups);
        self.matched = query;
    }

    /// The matches for what's in the box now, worked out here if the
    /// background search hasn't caught up (Return straight after a letter).
    fn groups_now(&mut self, cx: &App) -> Arc<Vec<Group>> {
        let query = self.query(cx);
        if query != self.matched {
            self.searching = None;
            let groups = self.catalog.groups(&query, &self.recent);
            self.show(query, groups);
        }
        self.groups.clone()
    }

    /// The app changed (a thread came or went, a badge moved): the list is
    /// made again from it shortly, and what's typed matched against that.
    fn app_changed(&mut self, cx: &mut Context<Self>) {
        if self.rebuilding.is_some() {
            return;
        }
        self.rebuilding = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REBUILD_EVERY).await;
            let _ = this.update(cx, |this, cx| {
                this.rebuilding = None;
                let Some(app) = this.app.upgrade() else { return };
                this.catalog = Arc::new(app.read(cx).anything_catalog());
                this.search(cx);
            });
        }));
    }

    /// ↑ and ↓ move the pick, stopping at the ends, and the list follows it.
    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let groups = self.groups_now(cx);
        self.selected = rules::step(&groups, self.selected.as_deref(), down);
        if let Some(index) = row_index(&groups, self.selected.as_deref()) {
            self.scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    /// The pick as things stand, for Return and the automation's dump.
    fn picked(&mut self, cx: &App) -> Option<Item> {
        let groups = self.groups_now(cx);
        rules::picked(&groups, self.selected.as_deref()).cloned()
    }

    /// Return (⌘Return for beside) on the pick.
    fn enter(&mut self, beside: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.picked(cx) {
            self.open(item, beside, window, cx);
        }
    }

    fn open(&mut self, item: Item, beside: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |t, cx| t.open_anything_item(item, beside, window, cx));
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |t, cx| t.close_open_anything(true, window, cx));
        }
    }
}

impl Threadbear {
    /// ⌘K: up, or down again if it's up.
    pub(crate) fn toggle_open_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.anything.is_some() {
            self.close_open_anything(true, window, cx);
            return;
        }
        let app = cx.entity();
        let (catalog, recent) = (self.anything_catalog(), self.anything_recent.clone());
        let view = cx.new(|cx| AnythingBox::new(&app, catalog, recent, window, cx));
        let before = window.focused(cx);
        let input = view.read(cx).input.clone();
        input.update(cx, |s, cx| s.focus(window, cx));
        crate::web_panels::keyboard_to_app(window);
        self.anything = Some(OpenAnything { view, before });
        cx.notify();
    }

    /// Down, and the keyboard back where it was if nothing was opened.
    pub(crate) fn close_open_anything(&mut self, give_back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.anything.take() else { return };
        if give_back {
            if let Some(before) = open.before {
                window.focus(&before, cx);
                crate::web_panels::keyboard_to_app(window);
            }
        }
        cx.notify();
    }

    /// The app's pages as it has them now, each with its key.
    fn anything_pages(&self) -> Vec<PageEntry> {
        let entry = |page: Page, title: &str, key: Option<ShortcutCommand>| PageEntry { page, title: title.to_owned(), shortcut: key.and_then(|k| self.shortcut_display(k)) };
        let mut pages = vec![entry(Page::Inbox, "Inbox", Some(ShortcutCommand::ShowInbox))];
        if self.schedules_supported {
            pages.push(entry(Page::Scheduled, "Scheduled", Some(ShortcutCommand::ShowScheduled)));
        }
        if self.projects_supported {
            pages.push(entry(Page::Projects, "Projects", Some(ShortcutCommand::ShowProjects)));
            pages.push(entry(Page::Librarian, "Librarian", Some(ShortcutCommand::ShowLibrarian)));
        }
        pages.push(entry(Page::Settings(None), "Settings", Some(ShortcutCommand::Settings)));
        pages.push(entry(Page::Usage, "Usage", None));
        pages.push(entry(Page::Updates, "Updates", None));
        if self.connectors_on() {
            pages.push(entry(Page::Connectors, "Connectors", None));
        }
        if self.remote.all_runners.len() > 1 {
            pages.push(entry(Page::Runners, "Runners", None));
        }
        pages
    }

    fn anything_catalog(&self) -> Catalog {
        let tabs: Vec<(String, String)> = crate::settings_window::Tab::ALL.iter().map(|t| (t.name().to_owned(), t.title().to_owned())).collect();
        Catalog::new(self.threads.values().map(|t| &**t), &self.projects.list, self.anything_pages(), &tabs, &threadbear_core::sidebar::local_now())
    }

    /// Something picked: the box goes and the thing opens, straight away.
    /// Beside is a thread in a panel of its own, or a panel next to the main
    /// one; for anything else it's the same as Return.
    pub(crate) fn open_anything_item(&mut self, item: Item, beside: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_open_anything(false, window, cx);
        let key = item.kept_key().to_owned();
        self.note_recent(key.clone(), cx);
        // In the activity log as the menu-like pick it is: which kind, and
        // whether beside, so `tb activity` shows how the box gets used.
        let kind = match item.kind {
            Kind::Thread => "a thread",
            Kind::Panel => "a panel",
            Kind::Project => "a project",
            Kind::App => "a page",
        };
        crate::activity::menu_chosen(Some(format!("Open Anything > {kind}{}", if beside { " beside" } else { "" })), self.selected.clone());
        match item.target {
            Target::Thread(id) if beside => self.open_thread_in_panel(id, cx),
            Target::Thread(id) => {
                self.reveal_thread(&id, cx);
                self.open(id, window, cx);
            }
            Target::Project(id) => self.open_project_page(&id, cx),
            Target::Page(page) => self.open_anything_page(page, window, cx),
            Target::Panel { id, thread, project } => self.open_anything_panel(&key, &id, thread, project, beside, window, cx),
        }
        cx.notify();
    }

    fn open_anything_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        let command = match page {
            Page::Inbox => Command::ShowInbox,
            Page::Scheduled => Command::ShowScheduled,
            Page::Projects => Command::ShowProjects,
            Page::Librarian => Command::ShowLibrarian,
            Page::Settings(tab) => {
                self.open_settings(tab.as_deref().and_then(crate::settings_window::Tab::from_name), cx);
                return;
            }
            Page::Usage => return self.show_activity_panel(shell::USAGE, cx),
            Page::Updates => return self.show_activity_panel(shell::UPDATES, cx),
            Page::Connectors => return self.show_activity_panel(shell::CONNECTORS, cx),
            Page::Runners => return self.show_activity_panel(shell::RUNNERS, cx),
        };
        self.run_command(command, window, cx);
    }

    /// One of the activity bar's panels, brought forward where it is, else
    /// under the threads as its button opens it (never closed, as a second
    /// click on the button would).
    fn show_activity_panel(&mut self, panel: &str, cx: &mut Context<Self>) {
        if self.dock.read(cx).layout().contains(panel) {
            self.dock.update(cx, |m, cx| m.show(panel, &[], DockDrop::window(DockEdge::Left), cx));
        } else {
            self.toggle_under_threads(panel, cx);
        }
    }

    /// A patch panel. Shown where it is when the open thread (or project
    /// page) has it from that place; otherwise on the project's page if its
    /// folder is in a project, which has its folders' panels on the rail,
    /// else on the newest thread that has it.
    #[allow(clippy::too_many_arguments)]
    fn open_anything_panel(&mut self, key: &str, id: &str, thread: String, project: Option<String>, beside: bool, window: &mut Window, cx: &mut Context<Self>) {
        let here = |this: &Self| this.pane_thread().is_some_and(|t| t.panels().iter().any(|p| p.id == id && rules::panel_key(p, &t) == key));
        if !here(self) {
            if let Some(project) = project.filter(|p| self.project(p).is_some()) {
                self.open_project_page(&project, cx);
            }
            if !here(self) {
                self.reveal_thread(&thread, cx);
                self.open(thread, window, cx);
            }
        }
        let shown = self.dock.read(cx).layout().contains(id);
        if beside && !shown {
            // Next to the main panel, as a thread's own panel goes.
            let drop = match self.dock.read(cx).layout().group_containing(shell::MAIN) {
                Some(main) => DockDrop::edge(main.id, DockEdge::Right),
                None => DockDrop::window(DockEdge::Right),
            };
            self.dock.update(cx, |m, cx| m.open(id, drop, cx));
        } else if shown {
            self.dock.update(cx, |m, cx| m.show(id, &[], DockDrop::window(DockEdge::Right), cx));
        } else {
            // Where its rail button puts it. A page still loading shows its
            // own spinner there.
            self.show_pane(id, cx);
        }
    }

    /// Recent: something opened, through the box or otherwise.
    pub(crate) fn note_recent(&mut self, key: String, cx: &mut Context<Self>) {
        if self.anything_recent.first() == Some(&key) {
            return;
        }
        self.anything_recent = rules::used(&key, &self.anything_recent);
        self.settings.saved.open_anything_recent = Some(self.anything_recent.clone());
        self.save_settings_soon(cx);
    }

    /// A patch panel that's come into sight (its rail button, an agent's
    /// show_panel, the box) goes in Recent too.
    pub(crate) fn note_panels_shown(&mut self, layout: &threadbear_dock::DockLayout, cx: &mut Context<Self>) {
        let Some(thread) = self.pane_thread() else {
            self.anything_panels_shown.clear();
            return;
        };
        let showing: std::collections::HashSet<String> = layout.panels().into_iter().filter(|p| Self::is_pane_section(p)).collect();
        for panel in thread.panels() {
            if showing.contains(&panel.id) && !self.anything_panels_shown.contains(&panel.id) {
                self.note_recent(rules::panel_key(&panel, &thread), cx);
            }
        }
        self.anything_panels_shown = showing;
    }

    /// The openAnything experiment is on for this Mac's runner: ⌘K, ⌘P and
    /// Go > Open Anything… only then. Settings' switch first, as it changes
    /// without a new hello, else the hello's list.
    pub(crate) fn open_anything_on(&self) -> bool {
        let id = threadbear_core::shortcuts::OPEN_ANYTHING_EXPERIMENT;
        let switch = self.runner_settings.as_ref().and_then(|s| s["experiments"].as_array()?.iter().find(|e| e["id"] == id).map(|e| e["on"].as_bool() == Some(true)));
        switch.unwrap_or_else(|| self.remote.experiments.iter().any(|e| e == id))
    }

    /// The box over the window, or None while it's down: the box's own view,
    /// which draws itself.
    pub(crate) fn render_open_anything(&mut self) -> Option<AnyElement> {
        self.anything.as_ref().map(|open| open.view.clone().into_any_element())
    }
}

impl Render for AnythingBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (input, scroll, catalog, groups) = (self.input.clone(), self.scroll.clone(), self.catalog.clone(), self.groups.clone());
        let picked = rules::picked(&groups, self.selected.as_deref()).map(|i| i.key.clone());
        let viewport = window.viewport_size();
        let zoom = style::zoomed(1.);
        let (window_w, window_h) = (f32::from(viewport.width) / zoom, f32::from(viewport.height) / zoom);
        let width = WIDTH.min(window_w - 48.).max(280.);
        let top = (window_h * TOP_SHARE).clamp(24., 140.);
        // The list scrolls past what fits above the window's bottom edge.
        let list_max = (window_h - top - 48. - 56. - 40.).max(ROW_HEIGHT * 3.);
        let hairline = style::hairline(cx);
        // Spelled out, as the picture viewer's backdrop is: `black().opacity()`
        // came out clear here.
        let dim = gpui_kit::hsla(0., 0., 0., if cx.theme().is_dark() { 0.45 } else { 0.18 });
        let fill = if cx.theme().is_dark() { style::over_background(style::primary_opacity(cx, 0.06), cx) } else { cx.theme().background };

        let query_row = div()
            .id("anything-query")
            .flex_none()
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(hairline)
            .text_size(px(size::BODY + 1.))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.step(false, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.step(true, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                this.enter(action.secondary, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.close(window, cx);
                cx.stop_propagation();
            }))
            .child(Input::new(&input).appearance(false).pl(px(0.)).text_size(px(size::BODY + 1.)));

        let list = self.render_list(&catalog, &groups, picked.as_deref(), &scroll, list_max, cx);
        let footer = h_flex()
            .flex_none()
            .gap(px(18.))
            .px(px(16.))
            .py(px(8.))
            .border_t_1()
            .border_color(hairline)
            .text_size(px(size::DETAIL))
            .text_color(style::secondary_text(cx))
            .child(key_note("↵", "open", cx))
            .child(key_note("⌘↵", "open beside", cx))
            .child(key_note("esc", "close", cx));
        let panel = v_flex()
            .id("open-anything")
            .w(px(width))
            .rounded(px(CORNER))
            .overflow_hidden()
            .bg(fill)
            .border_1()
            .border_color(hairline)
            .shadow_lg()
            .occlude()
            // A press inside isn't one outside.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(query_row)
            .child(list)
            .child(footer);
        // Anchored at the window's corner, as the picture viewer's
        // backdrop is: a bare absolute layer in a deferred element has
        // nothing to sit in, and its dim never showed.
        deferred(
            anchored().position(point(gpui_kit::px(0.), gpui_kit::px(0.))).child(
                div()
                    .id("open-anything-dim")
                    .relative()
                    .w(viewport.width)
                    .h(viewport.height)
                    .bg(dim)
                    .occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close(window, cx)))
                    .child(div().absolute().top(px(top)).left_0().right_0().flex().justify_center().child(panel)),
            ),
        )
        .with_priority(3)
    }
}

impl AnythingBox {
    fn render_list(&mut self, catalog: &Catalog, groups: &[Group], picked: Option<&str>, scroll: &ScrollHandle, max: f32, cx: &mut Context<Self>) -> AnyElement {
        if groups.is_empty() {
            return div().px(px(16.)).py(px(14.)).text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child("Nothing by that name. Try fewer letters.").into_any_element();
        }
        // Headings and rows are the list's children in turn, so a row's place
        // among them is what the scroll handle goes to (`row_index`).
        let mut children: Vec<AnyElement> = Vec::new();
        for group in groups {
            children.push(div().px(px(16.)).pt(px(10.)).pb(px(4.)).text_size(px(size::DETAIL)).font_weight(FontWeight::SEMIBOLD).text_color(style::secondary_text(cx)).child(group.title).into_any_element());
            for item in &group.items {
                let on = picked == Some(item.key.as_str());
                children.push(self.render_row(catalog, item, on, cx));
            }
        }
        div().id("anything-list").flex_none().max_h(px(max)).pb(px(6.)).overflow_y_scroll().track_scroll(scroll).children(children).into_any_element()
    }

    fn render_row(&mut self, catalog: &Catalog, item: &Item, on: bool, cx: &mut Context<Self>) -> AnyElement {
        // Grey, not the accent: Mike asked for a quiet pick.
        let (selected_fill, hover) = (style::primary_opacity(cx, 0.09), style::primary_opacity(cx, 0.05));
        let secondary = style::secondary_text(cx);
        let icon = match (&item.target, catalog.panel_thread(item.kept_key())) {
            (Target::Panel { id, .. }, Some(thread)) => match thread.panels().into_iter().find(|p| &p.id == id) {
                Some(panel) => match self.app.upgrade() {
                    Some(app) => crate::patch_icons::panel_face(&panel, thread, &app.read(cx).sidebar.project_icons, 12., 9., cx),
                    None => crate::patch_icons::glyph(item.symbol, 12., Some(secondary)),
                },
                None => crate::patch_icons::glyph(item.symbol, 12., Some(secondary)),
            },
            _ => crate::patch_icons::glyph(item.symbol, 12., Some(secondary)),
        };
        let tip: SharedString = row_tip(item).into();
        let picked = item.clone();
        let detail = item.detail.clone();
        h_flex()
            .id(SharedString::from(format!("anything-row-{}", item.key)))
            .test_support()
            .role(Role::ListItem).aria_selected(on).aria_label(tip.clone())
            .h(px(ROW_HEIGHT + 4.))
            .px(px(16.))
            .gap(px(10.))
            .cursor_pointer()
            .when(on, |s| s.bg(selected_fill))
            .when(!on, |s| s.hover(move |s| s.bg(hover)))
            .tip(tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open(picked.clone(), false, window, cx)))
            .child(div().flex_none().size(px(20.)).rounded(px(5.)).flex().items_center().justify_center().bg(style::primary_opacity(cx, 0.07)).child(icon))
            .child(div().min_w_0().truncate().text_size(px(size::LABEL + 1.)).text_color(cx.theme().foreground).child(item.title.clone()))
            .children(item.badge.map(|n| {
                div().flex_none().px(px(6.)).h(px(17.)).flex().items_center().rounded_full().bg(style::primary_opacity(cx, 0.08)).text_size(px(size::SMALL)).font_weight(FontWeight::SEMIBOLD).text_color(secondary).child(if n > 99 { "99+".to_owned() } else { n.to_string() })
            }))
            .child(div().flex_1().min_w(px(8.)))
            .when(!detail.is_empty(), |s| s.child(div().flex_none().max_w(px(260.)).truncate().text_size(px(size::DETAIL)).text_color(secondary).child(detail)))
            .into_any_element()
    }

}

impl Threadbear {
    /// `dump`'s box, while it's up: what's typed, the groups and their
    /// entries as each line reads, and the pick.
    #[cfg(feature = "automation")]
    pub(crate) fn open_anything_dump(&self, cx: &App) -> Value {
        let Some(open) = &self.anything else { return Value::Null };
        // What the box is showing, so a spec waits for the search to come
        // back rather than reading matches worked out here.
        let b = open.view.read(cx);
        let picked = rules::picked(&b.groups, b.selected.as_deref());
        json!({
            "query": b.query(cx),
            "matched": b.matched,
            "groups": b.groups.iter().map(|g| json!({ "title": g.title, "items": g.items.iter().map(|i| line(i)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "picked": picked.map(|i| i.title.clone()),
            "recent": self.anything_recent,
        })
    }
}

/// One entry as the dump and the specs read it: "YouTube comments 4 | yt-channel-manager".
#[cfg(feature = "automation")]
fn line(item: &Item) -> String {
    let mut text = item.title.clone();
    if let Some(n) = item.badge {
        text.push_str(&format!(" {n}"));
    }
    if !item.detail.is_empty() {
        text.push_str(&format!(" | {}", item.detail));
    }
    text
}

/// What hovering an entry says it does.
fn row_tip(item: &Item) -> String {
    match item.kind {
        Kind::Thread => format!("Open {} (⌘Return: in a panel of its own)", item.title),
        Kind::Panel => format!("Show the {} panel (⌘Return: beside this thread)", item.title),
        Kind::Project => format!("Open the {} project's page", item.title),
        Kind::App => format!("Go to {}", item.title),
    }
}

/// The child index of the picked row among the list's headings and rows.
fn row_index(groups: &[Group], selected: Option<&str>) -> Option<usize> {
    let key = rules::picked(groups, selected)?.key.clone();
    let mut index = 0;
    for group in groups {
        index += 1;
        for item in &group.items {
            if item.key == key {
                return Some(index);
            }
            index += 1;
        }
    }
    None
}

/// A key cap and what it does, along the foot.
fn key_note(key: &'static str, words: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .gap(px(6.))
        .child(div().px(px(5.)).py(px(1.)).rounded(px(4.)).border_1().border_color(style::hairline(cx)).bg(style::primary_opacity(cx, 0.05)).text_size(px(size::SMALL)).child(key))
        .child(words)
}

/// The script's `anything`: `{"open": true}` puts the box up (and false
/// down), `"text"` types in it as a person would, replacing what's there,
/// and `"pick": "<title>"` clicks that entry (`"beside": true` for ⌘Return).
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    if let Some(open) = cmd["open"].as_bool() {
        if open != app.read(cx).anything.is_some() {
            app.update(cx, |t, cx| t.toggle_open_anything(window, cx));
        }
    }
    if let Some(text) = cmd["text"].as_str() {
        let view = app.read(cx).anything.as_ref().map(|o| o.view.clone()).ok_or("the box isn't up")?;
        let input = view.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_value(text.to_owned(), window, cx));
        // set_value doesn't say Change, as a person's typing does.
        view.update(cx, |b, cx| b.typed(cx));
    }
    if let Some(title) = cmd["pick"].as_str() {
        let view = app.read(cx).anything.as_ref().map(|o| o.view.clone()).ok_or("the box isn't up")?;
        let item = view.update(cx, |b, cx| {
            let groups = b.groups_now(cx);
            rules::flat(&groups).into_iter().find(|i| i.title == title).cloned()
        });
        let item = item.ok_or_else(|| format!("nothing called {title:?} in the box"))?;
        let beside = cmd["beside"].as_bool().unwrap_or(false);
        app.update(cx, |t, cx| t.open_anything_item(item, beside, window, cx));
    }
    Ok(json!("ok"))
}
