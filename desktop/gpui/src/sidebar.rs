// The left side of the window, a port of the Swift app's SidebarView and its
// friends (InboxList, ThreadSection, SectionHeading, ThreadRowView, RowActions,
// ThreadMenu, SnoozeMenu, SubagentList, ArchiveRows, SearchField). What a row
// says comes from threadbear_core::sidebar and its sizes and colours from
// style.rs; this file draws them and answers clicks. Every click shows at once
// (`update_thread` changes the summary before the runner hears), and the one
// slow thing with no optimistic state, deleting a chat, shows a spinner in its
// place and stops taking clicks until it lands.
use crate::app::Threadbear;
use crate::shell::SidebarMode;
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use threadbear_core::inbox::{iso_time, InboxOrder, ThreadSummary};
use threadbear_core::sidebar::{self as rows, RowAction, RowLook, RowMode, Tile, TrailingKind};
use crate::tips::Tip as _;

type Clock = threadbear_core::chrono::DateTime<threadbear_core::chrono::Local>;

/// The Threads panel as a view of its own, so the dock can reuse its last
/// frame (`cached` in shell.rs). Scrolling the transcript redraws the window
/// every frame, and before this every frame rebuilt every row of the thread
/// list as well: with 60 threads, about half the frames on a 120 Hz screen
/// were late. Everything it draws is the app's state, so it draws again
/// whenever the app notifies, which the scroll doesn't.
pub(crate) struct SidebarView {
    app: WeakEntity<Threadbear>,
    _app_changed: Subscription,
}

impl SidebarView {
    pub(crate) fn new(app: &Entity<Threadbear>, cx: &mut Context<Self>) -> Self {
        let app_changed = cx.observe(app, |_, _, cx| cx.notify());
        Self { app: app.downgrade(), _app_changed: app_changed }
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else { return div().into_any_element() };
        let focus = app.read(cx).sidebar.focus.clone();
        let content = app.update(cx, |t, cx| t.render_threads_panel(window, cx));
        // A click anywhere in the panel that isn't a field gives it the
        // keyboard (GPUI focuses the innermost focusable thing pressed), as
        // clicking a row does the Swift list's.
        let (up, down, escape) = (self.app.clone(), self.app.clone(), self.app.clone());
        div()
            .size_full()
            .track_focus(&focus)
            .key_context(LIST_CONTEXT)
            .on_action(move |_: &SelectPreviousThread, window, cx| {
                up.update(cx, |t, cx| t.step_thread(-1, window, cx)).ok();
            })
            .on_action(move |_: &SelectNextThread, window, cx| {
                down.update(cx, |t, cx| t.step_thread(1, window, cx)).ok();
            })
            .on_action(move |_: &BackToInbox, window, cx| {
                escape.update(cx, |t, cx| t.list_escape(window, cx)).ok();
            })
            .child(content)
            .into_any_element()
    }
}

// The Threads panel's keys (`onMoveCommand`, `onExitCommand`).
actions!(thread_list, [SelectPreviousThread, SelectNextThread, BackToInbox]);

/// The Threads panel's key context: its keys work while it has the keyboard,
/// and a field in it (the search) keeps its own arrows and Esc.
const LIST_CONTEXT: &str = "ThreadList";

pub(crate) fn bind_keys(cx: &mut App) {
    static BOUND: std::sync::Once = std::sync::Once::new();
    BOUND.call_once(|| {
        cx.bind_keys([KeyBinding::new("up", SelectPreviousThread, Some(LIST_CONTEXT)), KeyBinding::new("down", SelectNextThread, Some(LIST_CONTEXT)), KeyBinding::new("escape", BackToInbox, Some(LIST_CONTEXT))]);
    });
}

impl Threadbear {
    /// The threads the arrow keys walk, top to bottom, as drawn: a folded
    /// section's rows aren't, unless a search draws them, and Archived only
    /// its few (`order` in SidebarView).
    pub(crate) fn arrow_order(&self, cx: &App) -> Vec<String> {
        let now = now_ms();
        let searching = !self.search.read(cx).value().trim().is_empty();
        let mut out = Vec::new();
        for section in self.shown_sections(cx, now) {
            if !searching && self.folded.contains(section.key) {
                continue;
            }
            let total = section.rows.len();
            let shown = if section.key == "archived" {
                let selected = self.selected.as_ref().and_then(|id| section.rows.iter().position(|t| &t.id == id));
                rows::archive_rows(total, self.sidebar.archive_limit, searching, selected).0
            } else {
                total
            };
            out.extend(section.rows.into_iter().take(shown).map(|t| t.id));
        }
        out
    }

    /// Up or down a thread from the open one (from nothing open, the first
    /// or the last), kept in view, with the keyboard staying in the list.
    pub(crate) fn step_thread(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != SidebarMode::Inbox {
            return;
        }
        let order = self.arrow_order(cx);
        let next = match self.selected.as_ref().and_then(|s| order.iter().position(|id| id == s)) {
            None if step >= 0 => order.first(),
            None => order.last(),
            Some(i) => order.get((i as i32 + step).clamp(0, order.len() as i32 - 1) as usize),
        };
        let Some(next) = next.cloned().filter(|n| self.selected.as_ref() != Some(n)) else { return };
        self.open(next.clone(), window, cx);
        // Opening puts the cursor in the thread's box; the arrows keep it here.
        window.focus(&self.sidebar.focus, cx);
        crate::web_panels::keyboard_to_app(window);
        self.reveal_row(&next, cx);
    }

    /// Esc in the Threads panel: the Scheduled, Projects or Librarian view
    /// goes back to the Inbox.
    fn list_escape(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != SidebarMode::Inbox {
            self.set_mode(SidebarMode::Inbox, cx);
        }
    }

    /// Scrolls the thread's row into view once it's drawn.
    pub(crate) fn reveal_row(&mut self, id: &str, cx: &mut Context<Self>) {
        *self.sidebar.reveal.borrow_mut() = Some((id.to_owned(), 0));
        cx.notify();
    }
}

/// A project's icon (protocol `ProjectIcon`): a picture, else a symbol.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectIcon {
    pub(crate) image_path: Option<String>,
    pub(crate) symbol: Option<String>,
}

/// What the new-thread page needs to know of a project (`Project`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectBrief {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) folders: Vec<String>,
}

/// Each project's id, title and folders from a `projects.list` answer.
pub(crate) fn project_briefs(projects: &Value) -> Vec<ProjectBrief> {
    projects
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(ProjectBrief {
                id: p["id"].as_str()?.to_string(),
                title: p["title"].as_str().unwrap_or("").to_string(),
                folders: p["folders"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
            })
        })
        .collect()
}

/// Each project's icon from a `projects.list` (or `projects.updated`) answer.
pub(crate) fn project_icons(projects: &Value) -> HashMap<String, ProjectIcon> {
    projects
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let id = p["id"].as_str()?.to_string();
            let icon = p.get("icon")?;
            let image_path = icon.get("image").and_then(|i| i.get("path")).and_then(Value::as_str).map(String::from);
            let symbol = icon.get("symbol").and_then(Value::as_str).map(String::from);
            Some((id, ProjectIcon { image_path, symbol }))
        })
        .collect()
}

/// A menu open over a row or a heading: its rows (for scripts) and the view.
pub(crate) struct OpenMenu {
    /// The thread whose row it belongs to, which keeps its buttons showing.
    pub(crate) thread: Option<String>,
    position: Point<Pixels>,
    view: Entity<PopupMenu>,
    /// What's on it, for scripts (`menu-popover`, `realMenu`).
    #[cfg_attr(not(feature = "automation"), allow(dead_code))]
    pub(crate) rows: Vec<MenuRow>,
    /// Hibernate choices ask first if the thread's busy: the moon's menu does,
    /// the row's right-click one doesn't (`SnoozeNSMenu`, `SnoozeMenuItems`).
    confirm_busy: bool,
    _dismissed: Subscription,
}

/// One line in a menu (Swift builds these as Buttons in `ThreadMenu`).
#[derive(Clone)]
pub(crate) struct MenuRow {
    pub(crate) title: String,
    symbol: Option<&'static str>,
    action: Option<MenuAction>,
    checked: bool,
    enabled: bool,
    children: Vec<MenuRow>,
    separator: bool,
}

impl MenuRow {
    pub(crate) fn item(title: impl Into<String>, symbol: &'static str, action: MenuAction) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: Some(action), checked: false, enabled: true, children: vec![], separator: false }
    }
    pub(crate) fn separator() -> Self {
        Self { title: String::new(), symbol: None, action: None, checked: false, enabled: false, children: vec![], separator: true }
    }
    pub(crate) fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
    pub(crate) fn submenu(title: impl Into<String>, symbol: &'static str, children: Vec<MenuRow>) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: None, checked: false, enabled: true, children, separator: false }
    }
    /// A line of the new-thread box's Runs on menu: `star` marks the default
    /// (`markDefault`'s ★, drawn on the title; this menu has no badge yet).
    /// A ticked line keeps its symbol: the menu draws the tick in a column of
    /// its own beside the icon, as macOS's do (desktop/vendor's mac_look.rs).
    pub(crate) fn runner(title: impl Into<String>, symbol: &'static str, checked: bool, star: bool, action: MenuAction) -> Self {
        let mut title: String = title.into();
        if star {
            title.push_str("  ★");
        }
        Self { title, symbol: Some(symbol), action: Some(action), checked, enabled: true, children: vec![], separator: false }
    }
    /// A folder on the runner a new thread will start on (Runs on's Folder).
    pub(crate) fn runner_folder(folder: String, checked: bool) -> Self {
        Self { title: folder.clone(), symbol: Some("folder"), action: Some(MenuAction::DraftFolder(folder)), checked, enabled: true, children: vec![], separator: false }
    }
}

/// What choosing a menu line does.
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    Star(bool),
    Archive,
    Unarchive,
    Wake,
    Hibernate(&'static str),
    HibernateUntilRestart,
    PickTime,
    WakeWhen,
    Delete,
    Rename,
    CopyId,
    Sort(InboxOrder),
    /// A thread in a panel of its own, and back (thread_panels.rs).
    OpenInPanel,
    MoveToMainPanel,
    /// The header's ... menu (header.rs).
    CopyTranscript,
    OpenOnWeb,
    ThreadProject { thread: String, add: Option<String>, remove: Option<String> },
    /// A project's menu, or one of a thread's that acts on projects (projects.rs).
    Project(crate::projects::ProjectAction),
    /// Runs on: where the new thread starts, its default, and a folder on
    /// the chosen runner (new_thread.rs).
    Runner(Option<String>),
    DefaultRunner(Option<String>),
    DraftFolder(String),
    /// Move to ▸ a runner, and Cancel Move once it's going (move_sheet.rs).
    MoveTo(String, String),
    CancelMove,
    /// A schedule's row menu, by its id (scheduled.rs).
    EditSchedule(String),
    RunSchedule(String),
    PauseSchedule(String, bool),
    DeleteSchedule(String),
}

/// The sidebar's own state; the app holds one of these (`Threadbear::sidebar`).
pub(crate) struct SidebarState {
    pub(crate) order: InboxOrder,
    /// The row under the pointer.
    pub(crate) hovered: Option<String>,
    /// As `hover` in the Swift app's automation: a row shown as if the
    /// pointer were over it, so scripts don't move a real pointer.
    pub(crate) hover_override: Option<String>,
    pub(crate) archive_limit: usize,
    pub(crate) menu: Option<OpenMenu>,
    /// Work on the way that has no optimistic state ("delete:<id>"): its
    /// control shows a spinner and stops taking clicks (`AppModel+Busy`).
    pub(crate) busy: HashSet<String>,
    /// The runner's first list has come; until then, loading rows.
    pub(crate) loaded: bool,
    /// Each project's icon by id, for the tiles (`projects.list`).
    pub(crate) project_icons: HashMap<String, ProjectIcon>,
    /// The projects themselves, for the new-thread page's chip.
    pub(crate) projects: Vec<ProjectBrief>,
    /// Headings folding or unfolding, by key (Swift's
    /// `withAnimation(.snappy(duration: 0.2))`): the rows ease shut or open
    /// while what's below slides with them.
    pub(crate) folding: HashMap<&'static str, Fold>,
    /// Each section's rows' height when last drawn, for a fold to run to or from.
    heights: HashMap<&'static str, std::rc::Rc<std::cell::Cell<f32>>>,
    fold_serial: u64,
    /// A fold was under way when the list was last drawn, and the sections
    /// folded then (row_motion.rs).
    was_folding: bool,
    was_folded: HashSet<&'static str>,
    pub(crate) scroll: ScrollHandle,
    /// The Threads panel's keyboard: a click in it takes it, and then the
    /// arrows move through the threads and Esc goes back to the Inbox
    /// (`ThreadList`'s focus).
    pub(crate) focus: FocusHandle,
    /// A row to bring into view once it's laid out: the arrows', Next Inbox
    /// Thread's or a thread chip's (`revealSelectionRequests`).
    pub(crate) reveal: crate::row_motion::Reveal,
    /// Where the rows were and where they're going (row_motion.rs).
    pub(crate) motion: std::rc::Rc<crate::row_motion::RowMotion>,
    /// A row swiped or dragged with the mouse (row_swipe.rs).
    pub(crate) gestures: crate::row_swipe::RowGestures,
    /// Where the Inbox's sort button was last drawn, for its menu to hang
    /// from the button, as a macOS pull-down does, wherever the click was.
    sort_button: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    _tick: Task<()>,
}

/// A heading's fold under way: opening or closing, and which toggle it is,
/// so a click mid-fold starts a new one rather than finishing the old.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fold {
    pub(crate) opening: bool,
    serial: u64,
}

/// How long a fold takes, as Swift's `.snappy(duration: 0.2)`.
const FOLD_TIME: std::time::Duration = std::time::Duration::from_millis(200);

impl SidebarState {
    pub(crate) fn new(cx: &mut Context<Threadbear>) -> Self {
        // Redraws once a second while something ticks (a working row's timer,
        // a sub-agent's duration), otherwise every 20 s so "5m" moves on.
        bind_keys(cx);
        let tick = cx.spawn(async move |this, cx| {
            let mut n = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                n = n.wrapping_add(1);
                let alive = this.update(cx, |t, cx| {
                    let ticking = t.threads.values().any(|t| t.busy() || t.subagents.iter().flatten().any(|s| s.get("status").and_then(Value::as_str) == Some("running")));
                    let twentieth = n.is_multiple_of(20);
                    if twentieth || ticking {
                        cx.notify();
                    }
                    // A hibernating thread waking changes the badge's count.
                    if twentieth {
                        t.refresh_dock_badge();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        SidebarState {
            order: InboxOrder::default(),
            hovered: None,
            hover_override: None,
            archive_limit: rows::ARCHIVE_FIRST,
            menu: None,
            busy: HashSet::new(),
            loaded: false,
            project_icons: HashMap::new(),
            projects: Vec::new(),
            folding: HashMap::new(),
            heights: HashMap::new(),
            fold_serial: 0,
            was_folding: false,
            was_folded: HashSet::new(),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            reveal: Default::default(),
            motion: Default::default(),
            gestures: Default::default(),
            sort_button: Default::default(),
            _tick: tick,
        }
    }
}

/// The colours a frame of the sidebar uses, picked once (`cx.theme()` borrows
/// the whole app, which the row listeners also need).
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) fg: Hsla,
    pub(crate) secondary: Hsla,
    pub(crate) faint: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) failed: Hsla,
    pub(crate) hover: Hsla,
    pub(crate) selected: Hsla,
    pub(crate) tile: Hsla,
    pub(crate) tile_empty: Hsla,
    pub(crate) tile_ring: Hsla,
    pub(crate) hairline: Hsla,
    pub(crate) field_fill: Hsla,
    pub(crate) separator: Hsla,
    pub(crate) icon_hover: Hsla,
    pub(crate) icon_pressed: Hsla,
    pub(crate) guide: Hsla,
    pub(crate) skeleton: Hsla,
    pub(crate) side: Hsla,
}

pub(crate) fn palette(cx: &App) -> Palette {
    let theme = cx.theme();
    Palette {
        fg: theme.sidebar_foreground,
        secondary: style::secondary_text(cx),
        faint: style::faint_text(cx),
        accent: style::accent(cx),
        failed: style::failed(cx),
        hover: style::sidebar_hover(cx),
        selected: style::sidebar_selected(cx),
        tile: style::primary_opacity(cx, 0.08),
        tile_empty: style::primary_opacity(cx, 0.04),
        tile_ring: theme.sidebar,
        hairline: style::primary_opacity(cx, 0.08),
        field_fill: style::primary_opacity(cx, 0.035),
        separator: theme.sidebar_border,
        icon_hover: style::primary_opacity(cx, 0.08),
        icon_pressed: style::primary_opacity(cx, 0.14),
        guide: style::primary_opacity(cx, 0.16),
        skeleton: style::primary_opacity(cx, 0.07),
        side: theme.sidebar,
    }
}

/// `SectionMetrics` etc: the sidebar's measurements at 100%.
pub(crate) mod metrics {
    /// From the sidebar's edges to a row's highlight.
    pub const OUTER_INSET: f32 = 8.;
    /// Inside a row's highlight, left of its text: wider than the right side
    /// to leave a gutter for the unread dot, so titles line up with or without.
    pub const LEADING: f32 = 18.;
    pub const TRAILING: f32 = 10.;
    pub const CORNER: f32 = 8.;
    pub const TILE: f32 = 30.;
    /// `SubagentRowView.trunk`: the tree line runs down the middle of the tile.
    pub const TRUNK: f32 = LEADING + 15.;
    pub const INDENT: f32 = 18.;
}
pub(crate) use metrics::*;

/// Under where a button was clicked, where a dropdown opens (`MenuAnchor`'s
/// `below`), so the first item isn't under the pointer as the mouse lifts.
pub(crate) fn below(position: Point<Pixels>) -> Point<Pixels> {
    point(position.x, position.y + px(12.))
}

/// An SF Symbol's icon, at a point size and in a colour.
pub(crate) fn icon(name: &str, size: f32, color: Hsla) -> Icon {
    icons::symbol(name).with_size(px(size)).text_color(color)
}

/// A button-like control: its tooltip, and the label scripts find it by, as
/// the Swift app's `.tip` is both. The macro keeps the two in one place.
macro_rules! tipped {
    ($el:expr, $id:expr, $tip:expr) => {{
        let tip_text: SharedString = $tip.into();
        let shown = tip_text.clone();
        crate::tips::Tip::tip($el.id($id).test_support().aria_label(tip_text), shown)
    }};
}
pub(crate) use tipped;

/// `RowIconButtonStyle`: a 22 pt borderless icon button with its own faint
/// hover square.
pub(crate) fn icon_button(id: impl Into<ElementId>, symbol: Icon, tip: impl Into<SharedString>, p: &Palette, on_press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let (hover, pressed, secondary, fg) = (p.icon_hover, p.icon_pressed, p.secondary, p.fg);
    tipped!(div().flex_none().size(px(22.)).rounded(px(5.)).flex().items_center().justify_center().hover(move |s| s.bg(hover).text_color(fg)).text_color(secondary), id, tip)
        .active(move |s| s.bg(pressed))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |ev, window, cx| on_press(ev, window, cx))
        .child(symbol)
}

impl Threadbear {
    /// `threads.update` for any thread, changing the summary here first:
    /// anything clicked shows at once, and the runner's answer (or its
    /// thread.updated) puts it right or rolls it back.
    pub(crate) fn update_thread(&mut self, id: &str, change: impl FnOnce(&mut ThreadSummary), params: Value, cx: &mut Context<Self>) {
        let before = self.threads.get(id).cloned();
        if let Some(t) = self.threads.get_mut(id) {
            change(t);
        }
        let mut params = params;
        params["threadId"] = json!(id);
        let client = self.client.clone();
        let remote = before.as_ref().is_some_and(|t| t.runner_id.is_some());
        let going = crate::call_for_thread(client, remote, "threads.update", params);
        cx.spawn(async move |this, cx| {
            let res = going.await;
            this.update(cx, |this, cx| {
                if let Err(e) = res {
                    this.list_error = Some(e.message);
                    if let Some(before) = before {
                        this.threads.insert(before.id.clone(), before);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The Starred, Inbox and Running rows in order (`sections.all`), for
    /// picking what to open when the open thread goes away.
    fn live_row_ids(&self, cx: &App, now: i64) -> Vec<String> {
        self.shown_sections(cx, now).into_iter().filter(|s| matches!(s.key, "starred" | "inbox" | "running")).flat_map(|s| s.rows.into_iter().map(|t| t.id)).collect()
    }

    /// The row after this one, or before it when it's the last
    /// (`AppModel.neighbour`).
    fn neighbour_of(&self, id: &str, cx: &App, now: i64) -> Option<String> {
        let ordered = self.live_row_ids(cx, now);
        let index = ordered.iter().position(|x| x == id)?;
        ordered.get(index + 1).or_else(|| index.checked_sub(1).and_then(|i| ordered.get(i))).cloned()
    }

    /// Moves off a thread that's leaving the list, if it's the open one.
    fn leave_selection(&mut self, id: &str, next: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        match next {
            Some(next) => self.open(next, window, cx),
            None => {
                self.selected = None;
                self.leave_pane(cx);
                cx.notify();
            }
        }
    }

    pub(crate) fn star_row(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let at = on.then(|| iso_time(now_ms()));
        self.update_thread(id, move |t| t.starred_at = at, json!({ "starred": on }), cx);
    }

    pub(crate) fn archive_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        // The open thread's page drops into the Archived heading, before the
        // thread moves and the next one opens (archive_flight.rs).
        self.launch_flight(&id, crate::archive_flight::Destination::Archive, cx);
        let at = iso_time(now_ms());
        // "Archived" and Undo, at the top of the panel (`ArchivedToast`).
        if let Some(title) = self.threads.get(&id).map(|t| t.title.clone()) {
            let was_open = self.selected.as_deref() == Some(id.as_str());
            self.show_archived_note(id.clone(), title, was_open, cx);
        }
        self.update_thread(&id, move |t| t.archived_at = Some(at), json!({ "archived": true, "archivedBy": "mike" }), cx);
        self.leave_selection(&id, next, window, cx);
    }

    pub(crate) fn unarchive_row(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_thread(id, |t| t.archived_at = None, json!({ "archived": false }), cx);
    }

    pub(crate) fn wake_row(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_thread(
            id,
            |t| {
                t.snoozed_until = None;
                t.snoozed_until_restart = None;
                t.wake_when = None;
            },
            json!({ "snoozedUntil": Value::Null }),
            cx,
        );
    }

    /// Snooze until a moment, moving off it if it's open (`AppModel.snooze`).
    pub(crate) fn snooze_row(&mut self, id: String, until_ms: i64, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        // The open thread's page sinks into the moon (archive_flight.rs).
        self.launch_flight(&id, crate::archive_flight::Destination::Hibernate, cx);
        let until = iso_time(until_ms);
        let local = until.clone();
        self.update_thread(
            &id,
            move |t| {
                t.snoozed_until = Some(local);
                t.wake_when = None;
            },
            json!({ "snoozedUntil": until }),
            cx,
        );
        self.leave_selection(&id, next, window, cx);
    }

    /// Asks whether a runner restart is waiting. The runner says so to every
    /// client while one is (`runner.notice`), and on connect there may be one.
    pub(crate) fn refresh_restart(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "runner.status", json!({})).await;
            this.update(cx, |this, cx| {
                let status = res.ok();
                // Which threads a restart waits for (restart.rs). A runner
                // older than runner.status: its notices are all there is.
                if let Some(status) = &status {
                    this.client.restart_status(&status["pendingRestart"]);
                }
                this.website_url = status.as_ref().and_then(crate::header::website_url);
                // Where Investigate's thread works (`ownCheckout`); kept from
                // the last answer if this one has none.
                if let Some(checkout) = status.as_ref().and_then(|s| s["update"]["checkout"].as_str()).filter(|c| !c.is_empty()) {
                    this.runner_checkout = Some(checkout.to_owned());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `refresh_restart` in a moment, once for a burst of thread updates
    /// (`refreshRunnerStatus`'s 300 ms).
    pub(crate) fn refresh_restart_soon(&mut self, cx: &mut Context<Self>) {
        self.restart_status_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
            this.update(cx, |this, cx| this.refresh_restart(cx)).ok();
        }));
    }

    /// A restart under way that goes on past its patience means the runner
    /// is down, not restarting: the banner says so then, with Try again
    /// (`restartExpiry`).
    pub(crate) fn watch_restart_expiry(&mut self, cx: &mut Context<Self>) {
        let Some(since) = self.client.restart().restarting_since() else { return };
        let left = threadbear_client::restart::PATIENCE.saturating_sub(since.elapsed()) + std::time::Duration::from_secs(1);
        self.restart_expiry = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(left).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        }));
    }

    /// Hibernate until the runner next starts (`snoozedUntil: "restart"`).
    pub(crate) fn snooze_until_restart(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        self.launch_flight(&id, crate::archive_flight::Destination::Hibernate, cx);
        self.update_thread(
            &id,
            |t| {
                t.snoozed_until = Some("9999-12-31T00:00:00.000Z".into());
                t.snoozed_until_restart = Some(true);
            },
            json!({ "snoozedUntil": "restart" }),
            cx,
        );
        self.leave_selection(&id, next, window, cx);
    }

    /// Hibernate until when Mike said, in his words (`AppModel.hibernateWhen`):
    /// away at once, saying Thinking… until the runner works out when; back
    /// with the runner's reason if it can't.
    pub(crate) fn hibernate_when(&mut self, id: String, said: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        self.launch_flight(&id, crate::archive_flight::Destination::Hibernate, cx);
        let before = self.threads.get(&id).cloned();
        if let Some(t) = self.threads.get_mut(&id) {
            t.snoozed_until = Some("9999-12-31T00:00:00.000Z".into());
            t.snoozed_until_restart = None;
            t.wake_when = Some(json!({ "said": said, "pending": true }));
        }
        self.leave_selection(&id, next, window, cx);
        let client = self.client.clone();
        let params = json!({ "threadId": id, "said": said });
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.hibernateWhen", params).await;
            this.update(cx, |this, cx| {
                if let Err(e) = res {
                    this.list_error = Some(e.message);
                    if let Some(before) = before {
                        this.threads.insert(before.id.clone(), before);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Delete from the sidebar, only for an archived chat, after asking
    /// (`AppModel.deleteThread`). Its trash button shows a spinner until the
    /// runner answers, so it can't be pressed twice.
    fn delete_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let busy_key = format!("delete:{id}");
        if self.sidebar.busy.contains(&busy_key) {
            return;
        }
        let Some(thread) = self.threads.get(&id) else { return };
        if !thread.is_archived() {
            return;
        }
        let title = format!("Delete \u{201C}{}\u{201D}?", thread.title);
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (entity, id, busy_key) = (entity.clone(), id.clone(), busy_key.clone());
            alert
                .title(title.clone())
                .description("This removes the chat from Threadbear and moves it to the runner's trash. It can't be undone in the app.")
                .show_cancel(true)
                .button_props(DialogButtonProps::default().ok_text("Delete").show_cancel(true))
                .on_ok(move |_, _, cx| {
                    let (id, busy_key) = (id.clone(), busy_key.clone());
                    entity
                        .update(cx, move |this, cx| {
                            // The row spins, and the banner says what's going
                            // on, as the click came from a menu (`whileBusy(note:)`).
                            if !this.begin_note(&busy_key, "Deleting chat…", cx) {
                                return;
                            }
                            this.sidebar.busy.insert(busy_key.clone());
                            cx.notify();
                            let client = this.client.clone();
                            let params = json!({ "threadId": id });
                            cx.spawn(async move |this, cx| {
                                let res = call(client, "threads.delete", params).await;
                                this.update(cx, |this, cx| {
                                    this.sidebar.busy.remove(&busy_key);
                                    this.end_note(&busy_key, cx);
                                    match res {
                                        Ok(_) => {
                                            this.threads.remove(&id);
                                        }
                                        Err(e) => this.list_error = Some(e.message),
                                    }
                                    cx.notify();
                                })
                                .ok();
                            })
                            .detach();
                        })
                        .ok();
                    true
                })
        });
    }

    /// A hover button's archive or hibernate on a thread with work going
    /// asks first (`BusyConfirm`); the menus and ⌘E are deliberate and don't.
    fn confirm_if_busy(&mut self, id: &str, archive: bool, window: &mut Window, cx: &mut Context<Self>, go: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static) {
        let Some(thread) = self.threads.get(id).cloned() else { return };
        if !rows::needs_busy_confirm(&thread) {
            go(self, window, cx);
            return;
        }
        let (title, detail) = rows::busy_prompt(&thread, archive);
        let verb = if archive { "Archive" } else { "Hibernate" };
        let entity = cx.entity().downgrade();
        let go = std::rc::Rc::new(go);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (entity, go) = (entity.clone(), go.clone());
            alert.title(title.clone()).description(detail.clone()).button_props(DialogButtonProps::default().ok_text(verb).show_cancel(true)).on_ok(move |_, window, cx| {
                let go = go.clone();
                entity.update(cx, move |this, cx| go(this, window, cx)).ok();
                true
            })
        });
    }

    /// Runs a menu line for a row (or the Inbox's sort).
    /// Runs a menu line for a row (or the Inbox's sort), after asking if the
    /// moon's menu is about to stop a running turn (`BusyConfirm`).
    pub(crate) fn run_menu_action(&mut self, thread: Option<String>, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        let confirm_busy = self.sidebar.menu.take().is_some_and(|m| m.confirm_busy);
        match action {
            MenuAction::Sort(order) => {
                self.sidebar.order = order;
                self.sidebar_settings_changed(cx);
                cx.notify();
                return;
            }
            MenuAction::Runner(runner) => return self.choose_new_thread_runner(runner, true, cx),
            MenuAction::DefaultRunner(runner) => return self.set_default_runner(runner, cx),
            MenuAction::DraftFolder(folder) => return self.change_new_draft(&json!({ "cwd": folder }), cx),
            MenuAction::EditSchedule(id) => return self.edit_schedule(&id, window, cx),
            MenuAction::RunSchedule(id) => return self.run_schedule(id, cx),
            MenuAction::PauseSchedule(id, paused) => return self.set_schedule_paused(id, paused, cx),
            MenuAction::DeleteSchedule(id) => return self.delete_schedule(id, window, cx),
            MenuAction::Project(action) => return self.run_project_action(action, window, cx),
            _ => {}
        }
        let Some(id) = thread else { return };
        if confirm_busy && matches!(action, MenuAction::Hibernate(_) | MenuAction::HibernateUntilRestart | MenuAction::PickTime | MenuAction::WakeWhen) {
            let (inner_id, inner_action) = (id.clone(), action.clone());
            self.confirm_if_busy(&id, false, window, cx, move |this, window, cx| this.run_row_action(inner_id.clone(), inner_action.clone(), window, cx));
            return;
        }
        self.run_row_action(id, action, window, cx);
    }

    fn run_row_action(&mut self, id: String, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            MenuAction::Star(on) => self.star_row(&id, on, cx),
            MenuAction::Archive => self.archive_row(id, window, cx),
            MenuAction::Unarchive => self.unarchive_row(&id, cx),
            MenuAction::Wake => self.wake_row(&id, cx),
            MenuAction::Hibernate(preset) => {
                let until = rows::snooze_date(preset, &rows::local_now());
                self.snooze_row(id, until, window, cx);
            }
            MenuAction::HibernateUntilRestart => self.snooze_until_restart(id, window, cx),
            MenuAction::PickTime => self.open_pick_time(id, window, cx),
            MenuAction::WakeWhen => self.open_wake_when(id, window, cx),
            MenuAction::Delete => self.delete_row(id, window, cx),
            // The bar's own title, opened on the thread and selected, as the
            // Swift menu's Rename does (`startRenaming`).
            MenuAction::Rename => self.start_renaming(id, window, cx),
            MenuAction::CopyId => cx.write_to_clipboard(ClipboardItem::new_string(id)),
            // BearDock isn't in this app yet, so a thread opens in the main pane.
            MenuAction::OpenInPanel => self.open_thread_in_panel(id, cx),
            MenuAction::MoveToMainPanel => self.move_to_main_panel(id, window, cx),
            MenuAction::CopyTranscript => {
                // The menu's thread: in the main panel, or a panel of its own.
                let main = self.pane.clone().filter(|p| p.read(cx).summary.id == id);
                if let Some(pane) = main.or_else(|| self.thread_panel_pane(&id)) {
                    pane.update(cx, |p, cx| p.copy_transcript(cx));
                }
            }
            MenuAction::OpenOnWeb => {
                if let Some(link) = self.thread_web_link(&id) {
                    crate::open_gate::open_web(&link, cx);
                }
            }
            MenuAction::ThreadProject { thread, add, remove } => self.set_thread_projects(thread, add, remove, cx),
            MenuAction::MoveTo(runner_id, runner_name) => self.open_move(crate::move_sheet::MoveRequest { thread_id: id, runner_id, runner_name }, window, cx),
            MenuAction::CancelMove => self.cancel_move(id, cx),
            MenuAction::Sort(_)
            | MenuAction::Runner(_)
            | MenuAction::DefaultRunner(_)
            | MenuAction::DraftFolder(_)
            | MenuAction::EditSchedule(_)
            | MenuAction::RunSchedule(_)
            | MenuAction::PauseSchedule(..)
            | MenuAction::DeleteSchedule(_)
            | MenuAction::Project(_) => {}
        }
        cx.notify();
    }

    /// `ThreadMenu`: the row's right-click menu.
    pub(crate) fn thread_menu_rows(&self, t: &ThreadSummary, cx: &App) -> Vec<MenuRow> {
        let mut rows = vec![
            // So several threads can be open at once (thread_panels.rs).
            if self.has_thread_panel(&t.id, cx) {
                MenuRow::item("Move to Main Panel", "rectangle.portrait.and.arrow.forward", MenuAction::MoveToMainPanel)
            } else {
                MenuRow::item("Open in New Panel", "rectangle.split.2x1", MenuAction::OpenInPanel)
            },
            MenuRow::separator(),
            MenuRow::item("Rename", "pencil", MenuAction::Rename),
            if t.is_starred() { MenuRow::item("Remove Star", "star.slash", MenuAction::Star(false)) } else { MenuRow::item("Star", "star", MenuAction::Star(true)) },
            MenuRow::submenu("Hibernate", "moon.zzz", self.snooze_menu_rows()),
        ];
        if t.snoozed_until.is_some() {
            rows.push(MenuRow::item("Wake", "sun.max", MenuAction::Wake));
        }
        if t.is_archived() {
            rows.push(MenuRow::item("Unarchive", "tray.and.arrow.up", MenuAction::Unarchive));
            rows.push(MenuRow::item("Delete…", "trash", MenuAction::Delete));
        } else {
            rows.push(MenuRow::item("Archive", "archivebox", MenuAction::Archive));
        }
        // Projects ▸ (`ThreadProjectsMenu`): projects are this Mac's runner's,
        // and another runner's threads aren't in them.
        if self.projects_supported && !t.is_remote() {
            rows.push(self.thread_projects_menu(t));
        }
        // Move to ▸ another runner (`ThreadMoveMenu`);
        // while a move's going, Cancel Move instead.
        let moving = t.moving.as_ref().filter(|m| !matches!(m["state"].as_str(), Some("cancelled") | Some("failed")));
        if moving.is_some() {
            rows.push(MenuRow::item("Cancel Move", "xmark.circle", MenuAction::CancelMove));
        } else {
            let targets = self.move_targets(t.runner_id.as_deref());
            if !targets.is_empty() {
                let children = targets
                    .into_iter()
                    .map(|r| {
                        let title = if r.online { r.name.clone() } else { format!("{}, offline", r.name) };
                        let mut row = MenuRow::item(title, crate::icons::runner_symbol(r.icon.as_deref()), MenuAction::MoveTo(r.id, r.name));
                        row.enabled = r.online;
                        row
                    })
                    .collect();
                rows.push(MenuRow::submenu("Move to", "arrow.right.square", children));
            }
        }
        // "Copy thread id" comes after a divider, as `.iconContextMenu` adds it.
        rows.push(MenuRow::separator());
        rows.push(MenuRow::item("Copy thread id", "number", MenuAction::CopyId));
        rows
    }

    /// `SnoozeMenuItems`/`SnoozeNSMenu`: the presets, then Pick a time and Wake when.
    fn snooze_menu_rows(&self) -> Vec<MenuRow> {
        let now = rows::local_now();
        let mut items: Vec<MenuRow> = rows::snooze_presets(&now).into_iter().map(|p| MenuRow::item(p.title, p.symbol(), MenuAction::Hibernate(p.id))).collect();
        // Only while a restart is waiting, so a thread holding it up can go
        // away now and come back after (`app.restartPending`).
        if self.client.restart().is_pending() {
            items.push(MenuRow::item(rows::SNOOZE_UNTIL_RESTART, "arrow.clockwise", MenuAction::HibernateUntilRestart));
        }
        items.push(MenuRow::separator());
        items.push(MenuRow::item(rows::SNOOZE_PICK_TITLE, "calendar", MenuAction::PickTime));
        items.push(MenuRow::item(rows::WAKE_MENU_TITLE, "text.bubble", MenuAction::WakeWhen));
        items
    }

    fn sort_menu_rows(&self) -> Vec<MenuRow> {
        InboxOrder::ALL
            .into_iter()
            .map(|order| {
                let mut row = MenuRow::item(order.title(), order.symbol(), MenuAction::Sort(order));
                row.checked = order == self.sidebar.order;
                row
            })
            .collect()
    }

    /// Opens `rows` as a popup menu at `position`, for a row or a heading.
    pub(crate) fn open_menu(&mut self, thread: Option<String>, menu_rows: Vec<MenuRow>, position: Point<Pixels>, confirm_busy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        let (build_rows, build_thread) = (menu_rows.clone(), thread.clone());
        let view = PopupMenu::build(window, cx, move |menu, window, cx| fill_menu(menu, &build_rows, entity.clone(), build_thread.clone(), window, cx));
        // The menu closes itself on Escape, a click outside or a choice.
        let dismissed = cx.subscribe(&view, |this, _, _: &DismissEvent, cx| {
            this.sidebar.menu = None;
            cx.notify();
        });
        self.sidebar.menu = Some(OpenMenu { thread, position, view, rows: menu_rows, confirm_busy, _dismissed: dismissed });
        cx.notify();
    }

    pub(crate) fn open_row_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.threads.get(&id).cloned() else { return };
        let menu_rows = self.thread_menu_rows(&t, cx);
        self.open_menu(Some(id), menu_rows, position, false, window, cx);
    }

    pub(crate) fn open_hibernate_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let menu_rows = self.snooze_menu_rows();
        self.open_menu(Some(id), menu_rows, position, true, window, cx);
    }

    /// "Wake when…": a text box for when, in Mike's own words (`WakeWhenPrompt`).
    pub(crate) fn open_wake_when(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        let field = cx.new(|cx| InputState::new(window, cx).placeholder(rows::WAKE_PLACEHOLDER));
        let field_focus = field.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (entity, field_read, id) = (entity.clone(), field.clone(), id.clone());
            dialog
                .title("Hibernate until")
                .width(px(460.))
                .child(Input::new(&field))
                .button_props(DialogButtonProps::default().ok_text("Hibernate").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let said = field_read.read(cx).value().trim().to_string();
                    let id = id.clone();
                    if !said.is_empty() {
                        entity.update(cx, |this, cx| this.hibernate_when(id, said, window, cx)).ok();
                    }
                    true
                })
        });
        let handle = field_focus.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        crate::web_panels::keyboard_to_app(window);
    }

    /// "Pick a time…": a calendar and a time, and a word if it's already
    /// gone (`SnoozePicker`).
    pub(crate) fn open_pick_time(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::date_picker::{DatePicker, DatePickerState};
        use gpui_kit::component::time_field::TimePrecision;
        use threadbear_core::chrono::{Duration as ChronoDuration, Local, NaiveDateTime, TimeZone as _};
        let entity = cx.entity().downgrade();
        let start: NaiveDateTime = (Local::now() + ChronoDuration::hours(3)).naive_local();
        let picker = cx.new(|cx| {
            let mut state = DatePickerState::new(window, cx).time_precision(TimePrecision::Minute);
            state.set_date_time(start, window, cx);
            state
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (entity, picker_read, id) = (entity.clone(), picker.clone(), id.clone());
            dialog
                .title("Hibernate until")
                .width(px(360.))
                .child(DatePicker::new(&picker))
                .button_props(DialogButtonProps::default().ok_text("Hibernate").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let when = picker_read.read(cx).date_time().start();
                    let Some(ms) = when.and_then(|w| Local.from_local_datetime(&w).earliest()).map(|d| d.timestamp_millis()) else { return true };
                    if ms <= now_ms() {
                        // An earlier time today would wake it straight back up.
                        window.push_notification("That time has already gone. Pick a later one.", cx);
                        return false;
                    }
                    let id = id.clone();
                    entity.update(cx, |this, cx| this.snooze_row(id, ms, window, cx)).ok();
                    true
                })
        });
    }

    /// The projects' icons, for the tiles: on connect and whenever a
    /// project changes (`projects.updated`).
    pub(crate) fn load_projects(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(v) = call(client, "projects.list", json!({ "archived": true })).await {
                this.update(cx, |this, cx| {
                    this.sidebar.project_icons = project_icons(&v["projects"]);
                    this.projects_supported = true;
                    this.librarian.projects_made = crate::librarian::projects_made(&v["projects"]);
                    // The librarian is part of projects, so it's asked after them.
                    this.load_librarian(cx);
                    this.all_projects = crate::app::project_list(&v["projects"]);
                    this.sidebar.projects = project_briefs(&v["projects"]);
                    this.receive_projects(&v["projects"]);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The whole list: search and new-thread line, then the sections.
    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let now = now_ms();
        let clock: Clock = rows::local_now();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let mut sections = self.shown_sections(cx, now);
        // A thread whose page is on its way to Archived or Hibernating isn't
        // listed or counted there until it lands (archive_flight.rs).
        for section in &mut sections {
            section.rows.retain(|t| !self.flight.holds(section.key, &t.id));
        }
        // Where flights can aim, noted afresh as the list draws.
        crate::archive_flight::sidebar_drawing();
        // When a schedule next messages each thread, for those under
        // Hibernating for it (`app.wakes`).
        let wakes = threadbear_core::inbox::wakes(&self.scheduled.list);
        // Rows glide to their new place as the list changes (row_motion.rs):
        // what's drawn, top to bottom, and what a fold is moving already.
        let motion = self.sidebar.motion.clone();
        let (mut drawn, mut quiet) = (Vec::<String>::new(), HashSet::<String>::new());
        let mut list = v_flex().px(px(OUTER_INSET)).pb(px(32.)).child(motion.list_marker());
        let loaded = self.sidebar.loaded;
        if !loaded {
            list = list.child(render_loading_rows(&p));
        } else if searching && sections.iter().all(|s| s.rows.is_empty()) {
            list = list.child(div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).text_color(p.secondary).child("No threads match."));
        } else {
            for section in &sections {
                // Empty sections other than Inbox are left out, and Inbox too
                // while searching (`ThreadSection`).
                if section.rows.is_empty() && (section.key != "inbox" || searching) {
                    continue;
                }
                let folded = !searching && self.folded.contains(section.key);
                let total = section.rows.len();
                // The Archived section shows the most recent few (`ArchiveRows`).
                let shown = if section.key == "archived" {
                    let selected = self.selected.as_ref().and_then(|id| section.rows.iter().position(|t| &t.id == id));
                    rows::archive_rows(total, self.sidebar.archive_limit, searching, selected)
                } else {
                    (total, 0)
                };
                let heading_key = format!("heading:{}", section.key);
                let heading = self.render_heading(section.key, section.title, total, folded, searching, &p, cx).into_any_element();
                list = list.child(motion.row(&heading_key, heading));
                drawn.push(heading_key);
                // A fold under way draws the rows while they ease shut, and
                // from the moment they start to open.
                let fold = (!searching).then(|| self.sidebar.folding.get(section.key).copied()).flatten();
                if folded && fold.is_none() {
                    continue;
                }
                let mut body = v_flex().relative().w_full();
                let mut place = |key: String, element: AnyElement, body: Div| {
                    // A section folding or unfolding moves its rows itself.
                    if fold.is_some() {
                        quiet.insert(key.clone());
                    }
                    let glided = motion.row(&key, element);
                    drawn.push(key);
                    body.child(glided)
                };
                for t in section.rows.iter().take(shown.0) {
                    let wake = (section.key == "hibernating").then(|| wakes.get(&t.id)).flatten();
                    let mut row = self.render_row(t, section.key, wake, now, &clock, &p, cx);
                    // The open row closes up as its page flies, lifting the
                    // heading below it, which the flight allows for.
                    if self.selected.as_ref() == Some(&t.id) {
                        row = div().relative().child(row).child(crate::archive_flight::mark(crate::archive_flight::Mark::OpenRow)).into_any_element();
                    }
                    body = place(t.id.clone(), row, body);
                    if !matches!(section.key, "hibernating" | "archived") {
                        for (i, sub) in self.render_subagents(t, section.key, now, &p, cx).into_iter().enumerate() {
                            body = place(format!("sub:{}:{i}", t.id), sub, body);
                        }
                    }
                }
                if section.key == "archived" && shown.1 > 0 {
                    let more = self.render_show_more(shown.1, &p, cx).into_any_element();
                    body = place("more:archived".into(), more, body);
                }
                // Under the empty Inbox heading.
                if section.key == "inbox" && section.rows.is_empty() && !searching {
                    let empty = div().pl(px(LEADING)).pt(px(14.)).text_size(px(style::size::SIDEBAR_PREVIEW)).text_color(p.secondary).child("Nothing in the inbox.").into_any_element();
                    body = place("note:inbox-empty".into(), empty, body);
                }
                // Its height as drawn, for a fold to run to or from.
                let height = self.sidebar.heights.entry(section.key).or_default().clone();
                let measured = height.clone();
                body = body.child(canvas(move |bounds, _, _| measured.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().top_0().left_0().size_full());
                list = list.child(match fold {
                    None => body.into_any_element(),
                    Some(fold) => div()
                        .w_full()
                        .overflow_hidden()
                        .child(body)
                        .with_animation(("section-fold", fold.serial as usize), Animation::new(FOLD_TIME).with_easing(ease_out_quint()), move |el, t| {
                            let open = if fold.opening { t } else { 1. - t };
                            el.h(gpui_kit::px(height.get() * open)).opacity(open)
                        })
                        .into_any_element(),
                });
            }
        }
        // A fold moves the rows below it as its height eases, so nothing
        // glides while one's under way, nor as it ends and its rows go: the
        // rest are where they'll stay by then. One with no easing (a script's,
        // or the folds coming from elsewhere) puts them there at once, as the
        // Swift app's does. Left to glide, the rows under a folded Inbox slid
        // up from where they'd last been drawn for half a second, and a
        // scripted click on one of them missed it.
        let folding = !self.sidebar.folding.is_empty();
        let was_folding = std::mem::replace(&mut self.sidebar.was_folding, folding);
        let refolded = self.sidebar.was_folded != self.folded;
        if refolded {
            self.sidebar.was_folded = self.folded.clone();
        }
        if folding || was_folding || refolded {
            quiet.extend(drawn.iter().cloned());
        }
        if loaded {
            motion.rows_drawn(drawn, &quiet, cx.reduce_motion());
        }
        // Last in the list, so it's laid out after every row (row_motion.rs).
        if self.sidebar.reveal.borrow().is_some() {
            list = list.child(motion.reveal_marker(self.sidebar.reveal.clone(), self.sidebar.scroll.clone(), self.sidebar_view.as_ref().map(|v| v.entity_id())));
        }
        let new_thread_active = self.selected.is_none();
        let list_error = self.list_error.clone();
        let red = p.failed;
        v_flex()
            .relative()
            .size_full()
            .bg(p.side)
            // One line: search and New thread (Mike asked 30 Oct).
            .child(h_flex().gap(px(6.)).px(px(OUTER_INSET)).pt(px(4.)).items_stretch().child(self.render_search(&p, cx)).child(self.render_new_thread(new_thread_active, &p, cx)))
            // Nothing about the connection here: the banner over the thread
            // says it, as the Swift app's does, and only once it's worth
            // saying (`connection_banner`). The raw error showed here in the
            // middle of a restart (Mike, 12 Nov).
            .when_some(list_error, |s, e| s.child(div().mx(px(OUTER_INSET)).mt_2().text_sm().text_color(red).child(e)))
            .child(
                // The scrolling list, with no scrollbar (the times stay put),
                // fading out at the foot rather than cutting through a row.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    // A flight with its heading out of sight falls off the list's bottom.
                    .child(crate::archive_flight::mark(crate::archive_flight::Mark::List))
                    .child(div().id("sidebar-list").size_full().overflow_y_scroll().track_scroll(&self.sidebar.scroll).child(list))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(34.)).bg(linear_gradient(180., linear_color_stop(p.side.opacity(0.), 0.), linear_color_stop(p.side, 0.88)))),
            )
    }

    /// The open row or heading menu, drawn over the whole window rather than
    /// inside the sidebar panel (the dock clips what a panel draws).
    /// Where the open menu is, so a web panel's page (a native view, which
    /// it can't draw over) gets out of its way (web_panels.rs).
    pub(crate) fn open_menu_at(&self) -> Option<Point<Pixels>> {
        self.sidebar.menu.as_ref().map(|m| m.position)
    }

    pub(crate) fn render_open_menu(&self) -> Option<AnyElement> {
        crate::tips::menu_up(1, self.sidebar.menu.is_some());
        self.sidebar.menu.as_ref().map(|m| deferred(anchored().position(m.position).snap_to_window_with_margin(px(8.)).child(div().occlude().child(m.view.clone()))).with_priority(1).into_any_element())
    }

    fn render_search(&mut self, p: &Palette, _cx: &mut Context<Self>) -> impl IntoElement {
        // A field, not a row: a lighter fill and a hairline (`SearchField`).
        // The glass and the prompt sit on the same columns as a button row's
        // icon and title, so the Input's own padding is dropped.
        h_flex()
            .id("search-field")
            .tip("Search threads")
            .flex_1()
            .min_w_0()
            // 29 tall as the Swift field is (6 over and under a 14 pt line):
            // the kit's small input, a little shorter, sits in the middle.
            .h(px(29.))
            .pl(px(LEADING - 2.))
            .pr(px(9.))
            .gap(px(8.))
            .items_center()
            .rounded(px(CORNER))
            .bg(p.field_fill)
            .border(px(0.5))
            .border_color(p.separator)
            .child(div().flex_none().w(px(16.)).flex().justify_center().child(icon("magnifyingglass", 12.5, p.faint)))
            .child(div().flex_1().min_w_0().child(Input::new(&self.search).appearance(false).small().cleanable(true).pl(px(0.))))
    }

    /// `NewThreadButton`: the compose icon, lit while the new thread's
    /// composer is showing.
    fn render_new_thread(&mut self, active: bool, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, selected, fg, secondary) = (p.hover, p.selected, p.fg, p.secondary);
        let color = if active { fg } else { secondary };
        tipped!(div().w(px(32.)).flex_none().flex().items_center().justify_center().rounded(px(CORNER)).when(active, |s| s.bg(selected)).when(!active, |s| s.hover(move |h| h.bg(hover).text_color(fg))), "new-thread", "Start a new thread")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.new_thread(cx)))
            .child(icon("square.and.pencil", style::size::SIDEBAR_BUTTON, color))
            // Its key over the end of the search while ⌘ is held, as the
            // sidebar's edge is just right of it (key_hints.rs).
            .relative()
            .children(crate::key_hints::hint(self.shortcut_display(threadbear_core::shortcuts::ShortcutCommand::NewThread), crate::key_hints::Placement::Before))
    }

    /// `app.newThread()`: leaves the open thread for a fresh start, the
    /// cursor in the new box.
    pub(crate) fn new_thread(&mut self, cx: &mut Context<Self>) {
        self.leave_pane(cx);
        self.selected = None;
        self.shows_home = false;
        self.projects.open = None;
        self.note_navigation();
        self.new_thread.picks.project = None;
        self.new_thread.focus_pending = true;
        cx.notify();
    }

    /// An inbox heading: its symbol, folding the section, and the Inbox's
    /// sort button at its end.
    fn render_heading(&mut self, key: &'static str, title: &'static str, count: usize, folded: bool, searching: bool, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let symbol = match key {
            "starred" => "star",
            "inbox" => "tray",
            "running" => "bolt",
            "hibernating" => "moon.zzz",
            _ => "archivebox",
        };
        let trailing = (key == "inbox").then(|| self.render_sort_button(p, cx).into_any_element());
        self.section_heading(
            key,
            title,
            symbol,
            count,
            folded,
            !searching,
            trailing,
            p,
            move |this, cx| {
                let opening = this.folded.remove(key);
                if !opening {
                    this.folded.insert(key);
                }
                this.start_fold(key, opening, cx);
                this.sidebar_settings_changed(cx);
            },
            cx,
        )
    }

    /// Starts a heading's fold, and ends it once it's run, so a closed
    /// section stops drawing its rows (a click mid-fold starts another).
    fn start_fold(&mut self, key: &'static str, opening: bool, cx: &mut Context<Self>) {
        self.sidebar.fold_serial += 1;
        let serial = self.sidebar.fold_serial;
        self.sidebar.folding.insert(key, Fold { opening, serial });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FOLD_TIME + std::time::Duration::from_millis(30)).await;
            this.update(cx, |this, cx| {
                if this.sidebar.folding.get(key).is_some_and(|f| f.serial == serial) {
                    this.sidebar.folding.remove(key);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// `SectionHeading`: a quiet divider with room above each group, folding
    /// its rows away when clicked (unless `foldable` is false, as while
    /// searching). Folded, it says how many. The Scheduled list's headings
    /// are these too (scheduled.rs).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn section_heading(&mut self, key: &str, title: &str, symbol: &'static str, count: usize, folded: bool, foldable: bool, trailing: Option<AnyElement>, p: &Palette, on_toggle: impl Fn(&mut Threadbear, &mut Context<Threadbear>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("heading-{key}"));
        let (fg, secondary, faint, hairline) = (p.fg, p.secondary, p.faint, p.hairline);
        let chevron_group = group.clone();
        let hover_group = group.clone();
        let title = SharedString::from(title.to_owned());
        // Folded, its count rolls to a new number as threads come and go
        // (row_motion.rs), as the Swift heading's does.
        let count_line = style::natural_line(style::size::SIDEBAR_META);
        let count_el = (folded && count > 0).then(|| crate::row_motion::rolling(&format!("heading-{key}"), count, count_line, cx.reduce_motion(), |n| div().child(n.to_string()).into_any_element()));
        let label = h_flex()
            .group(group.clone())
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .pl(px(LEADING))
            .pt(px(18.))
            .pb(px(6.))
            .items_center()
            .child(match key {
                // Where an archived or hibernated thread's page lands, bouncing
                // as it does (archive_flight.rs).
                "archived" | "hibernating" => {
                    let kind = if key == "archived" { crate::archive_flight::Mark::ArchiveBox } else { crate::archive_flight::Mark::HibernateBox };
                    div().relative().flex_none().child(self.flight.landing_icon(key, icon(symbol, 11., secondary), secondary, fg, cx.reduce_motion())).child(crate::archive_flight::mark(kind)).into_any_element()
                }
                _ => icon(symbol, 11., secondary).flex_none().into_any_element(),
            })
            // Brighter under the pointer, as `SectionHeading`'s title is.
            .child(style::group_hover_text(div().id(SharedString::from(format!("heading-title-{key}"))).flex_none().text_size(px(style::size::SIDEBAR_HEADING)).line_height(px(16.)).font_semibold().text_color(secondary), hover_group, fg).child(title.clone()))
            .child(div().flex_1().min_w_0().h(px(1.)).mx(px(4.)).bg(hairline))
            .when_some(count_el, |s, n| s.child(div().flex_none().text_size(px(style::size::SIDEBAR_META)).text_color(faint).child(n)))
            .when(foldable, |s| {
                s.child(div().flex_none().when(!folded, |d| d.invisible()).group_hover(chevron_group, |d| d.visible()).child(icon(if folded { "chevron.right" } else { "chevron.down" }, 9., faint)))
            });
        h_flex()
            .pr(px(TRAILING))
            .items_center()
            .child(if !foldable {
                label.into_any_element()
            } else {
                // The padding is inside the fold button, so the whole band
                // above the rows folds.
                let tip = if folded { format!("Show {title}") } else { format!("Hide {title}") };
                tipped!(div().flex_1().min_w_0(), SharedString::from(format!("fold-{key}")), tip)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        on_toggle(this, cx);
                        cx.notify();
                    }))
                    .child(label)
                    .into_any_element()
            })
            .children(trailing)
            .into_any_element()
    }

    /// `InboxOrderMenu`: the sort button at the end of the Inbox heading.
    fn render_sort_button(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let order = self.sidebar.order;
        let color = if order == InboxOrder::NeedsYou { p.faint } else { p.secondary };
        let fg = p.fg;
        let placed = self.sidebar.sort_button.clone();
        // A hover brightens the arrows and draws no square behind them, as
        // `InboxOrderMenu`'s `hovering` does.
        tipped!(div().flex_none().mt(px(12.)).size(px(16.)).flex().items_center().justify_center().text_color(color).hover(move |s| s.text_color(fg)), "sort-inbox", format!("Sort: {}", order.title()))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                let menu_rows = this.sort_menu_rows();
                // As the Swift app's pull-down (a SwiftUI Menu) hangs: its
                // edge a point left of the button's and 9 below it,
                // measured off macOS 26's screen (features/tools/menu-shot.ts).
                let button = this.sidebar.sort_button.get();
                let at = point(button.left() - px(1.), button.bottom() + px(9.));
                this.open_menu(None, menu_rows, at, false, window, cx);
            }))
            .child(canvas(move |bounds, _, _| placed.set(bounds), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            // In the button's colour, so the hover above brightens it.
            .child(icons::symbol("arrow.up.arrow.down").with_size(px(10.)))
    }

    fn render_show_more(&mut self, more: usize, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let step = more.min(rows::ARCHIVE_STEP);
        let tip = format!("{more} older archived {} not shown", if more == 1 { "thread" } else { "threads" });
        let (hover, secondary, fg) = (p.hover, p.secondary, p.fg);
        tipped!(
            h_flex().gap(px(8.)).pl(px(LEADING - 2.)).pr(px(TRAILING)).py(px(7.)).rounded(px(CORNER)).hover(move |s| s.bg(hover)),
            "archive-show-more",
            tip
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.sidebar.archive_limit += rows::ARCHIVE_STEP;
            cx.notify();
        }))
        .child(div().w(px(16.)).child(icon("ellipsis", style::size::SIDEBAR_BUTTON, secondary)))
        .child(div().text_size(px(style::size::SIDEBAR_BUTTON)).text_color(fg).child(format!("Show {step} more")))
    }

    /// `ThreadRowView`: tile, title, place line, status or hover buttons.
    pub(crate) fn render_row(&mut self, t: &ThreadSummary, section: &'static str, wake: Option<&threadbear_core::inbox::Wake>, now: i64, clock: &Clock, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mode = RowMode::of(section, t, now);
        // The Hibernating and Archived rows read plainly; only live ones say
        // what they're doing (`ThreadRowView.body`).
        let look = if mode == RowMode::Inbox { RowLook::of(t, Some(section)) } else { RowLook::PLAIN };
        let trailing = rows::trailing(t, &look, mode, clock, wake);
        let id = t.id.clone();
        // Not while a page is in the thread's place: it isn't what's showing.
        let selected = self.selected.as_deref() == Some(id.as_str()) && self.home_page().is_none() && self.projects.open.is_none();
        // A script's hover wins over the real pointer, which on a shared
        // desktop can be resting on another row (`ThreadRowView.pointerOver`).
        let hovered_row = self.sidebar.hover_override.as_deref().or(self.sidebar.hovered.as_deref());
        // Not while the row's being swiped: they'd ride along with it.
        let show_buttons = (hovered_row == Some(id.as_str()) || self.sidebar.menu.as_ref().is_some_and(|m| m.thread.as_deref() == Some(id.as_str()))) && !self.sidebar.gestures.is_moving(&id);
        // What a swipe left does to it, if anything (row_swipe.rs).
        let swipe_action = crate::row_swipe::SwipeAction::of(mode);
        let title = if t.title.is_empty() { "Untitled".to_string() } else { t.title.clone() };
        let tip = format!("{title}\n{}", abbreviate_home(&t.cwd));
        let (bg_hover, bg_sel, fg, secondary) = (p.hover, p.selected, p.fg, p.secondary);

        let hover_id = id.clone();
        let menu_id = id.clone();
        let select_id = id.clone();

        // The tile with the unread dot or tick in the gutter to its left.
        let behind = if selected { p.tile_ring.blend(p.selected) } else if show_buttons { p.tile_ring.blend(p.hover) } else { p.tile_ring };
        let project_icon = t.projects.as_ref().and_then(|ps| ps.first()).and_then(|first| self.sidebar.project_icons.get(&first.id));
        // Another runner's thread with no project shows its machine's icon
        // where the folder would be (`ProjectTiles.runnerIcon`) and its name
        // in the tile's tooltip, faded when that machine's offline
        // (`ThreadRowView.body`).
        let runner_tile = t.runner_id.as_deref().filter(|_| t.projects.as_ref().is_none_or(|ps| ps.is_empty())).map(|id| (self.runner_icon_symbol(Some(id)), self.runner_place_of(id)));
        let tile = render_tile(t, &look, behind, project_icon, runner_tile, p);
        let offline = t.runner_id.as_deref().is_some_and(|id| self.remote.runners.iter().any(|r| r.id == id && !r.online));

        let byline = rows::byline(t);
        let place = t.place();
        let runner_label = self.runner_label(t);
        let place_tip_text = if byline.is_some() { byline.as_ref().unwrap().tip.clone() } else { rows::place_tip(t) };
        let place_id: ElementId = ElementId::Name(format!("place-{id}").into());
        // The hover buttons' room: 24 a button, as the Swift row keeps
        // beside its title, which takes in the 2 between them and leaves a
        // little gap before the first.
        let actions_room = mode.actions().len() as f32 * 24.;

        let title_el = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(style::size::SIDEBAR_TITLE))
            .line_height(px(17.))
            .text_color(fg)
            .when(look.emphasised, |s| s.font_semibold())
            .child(title);

        let second_line = h_flex()
            .items_baseline()
            .gap(px(6.))
            .child(
                h_flex()
                    .id(place_id)
                    .min_w_0()
                    .items_baseline()
                    .gap(px(4.))
                    .text_color(secondary)
                    .tip(place_tip_text.clone())
                    .when_some(byline.as_ref(), |s, b| s.child(icons::try_symbol(&b.symbol).unwrap_or_else(|| icons::symbol("wrench.and.screwdriver")).with_size(px(style::size::SIDEBAR_META)).text_color(secondary).flex_none()))
                    .child(div().min_w_0().truncate().text_size(px(style::size::SIDEBAR_META + 1.)).line_height(px(15.)).child(place)),
            )
            // Its own text after the place, kept whole while a long project
            // name is cut short, and capped, as a runner's name can be any
            // length (`CappedWidth(limit: 100)` in ThreadRowView).
            .when_some(runner_label, |s, runner| {
                let tip = format!("On {runner}");
                s.child(
                    div()
                        .id(ElementId::Name(format!("runner-{id}").into()))
                        .flex_none()
                        .max_w(px(100.))
                        .truncate()
                        .ml(px(-2.))
                        .text_size(px(style::size::SIDEBAR_META + 1.))
                        .line_height(px(15.))
                        .text_color(secondary)
                        .tip(tip.clone())
                        .child(format!("\u{2022} {runner}")),
                )
            })
            // With the hover buttons up, room for them here as on the title's
            // line: they're centred on the row, over both lines, so a long
            // project line ran on under them (Mike, 12 Nov). It's cut short
            // before them instead.
            .child(div().flex_1().min_w(px(if show_buttons { actions_room } else { 6. })))
            .when(!show_buttons, |s| s.child(render_trailing(&trailing, id.clone(), p)));

        let actions = mode.actions();
        let buttons = if show_buttons {
            Some(self.render_row_actions(t, actions, cx))
        } else {
            None
        };

        let row = h_flex()
            .id(ElementId::Name(format!("row-{id}").into()))
            .test_support()
            .aria_label(tip.clone())
            .tip(tip.clone())
            .when(offline, |s| s.opacity(0.5))
            .relative()
            .items_center()
            .gap(px(10.))
            .pt(px(8.))
            .pb(px(8.))
            .pl(px(LEADING))
            .pr(px(TRAILING))
            .rounded(px(CORNER))
            .when(selected, |s| s.bg(bg_sel))
            .when(!selected && show_buttons, |s| s.bg(bg_hover))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.sidebar.hovered = Some(hover_id.clone());
                } else if this.sidebar.hovered.as_deref() == Some(hover_id.as_str()) {
                    this.sidebar.hovered = None;
                }
                this.sidebar.gestures.row_hovered(&hover_id, *hovered, swipe_action);
                cx.notify();
            }))
            // Selects on mouse down, as Mail does (`ThreadRowView`); a click
            // waits for the mouse to come back up. The press is watched to see
            // whether it's a drag left (the swipe) or the thread dragged out.
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                let before = this.place();
                this.row_pressed(&select_id, swipe_action, ev.position, before);
                this.open(select_id.clone(), window, cx);
            }))
            // Dragged any way but left, it's the thread, to drop into a
            // thread's message as its chip (`drag.threadout`).
            .on_drag(self.dragged_thread(&id), |drag, grabbed, _, cx| crate::row_swipe::RowDragChip::new(drag, grabbed, cx))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                // The row's menu alone: the press went on to the sidebar
                // panel's own menu (Hide Title, Open in New Window), which
                // came up over it from a real right-click (12 Nov).
                cx.stop_propagation();
                this.open_row_menu(menu_id.clone(), ev.position, window, cx)
            }))
            .child(tile)
            .child(v_flex().flex_1().min_w_0().gap(px(3.)).child(h_flex().child(title_el).when(show_buttons, |s| s.child(div().flex_none().w(px(actions_room)).h(px(1.))))).child(second_line))
            .when_some(buttons, |s, b| s.child(h_flex().absolute().right(px(TRAILING - 5.)).top_0().bottom_0().items_center().gap(px(2.)).child(b)))
            .into_any_element();
        self.swipe_slot(&id, swipe_action, row, cx)
    }

    /// `RowActions`: the small buttons a row shows on hover in place of its
    /// time. Star, the moon's menu, archive; or unarchive and delete.
    fn render_row_actions(&mut self, t: &ThreadSummary, actions: &[RowAction], cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let id = t.id.clone();
        let mut row = h_flex().gap(px(2.));
        for action in actions {
            let tip = action.tip(t);
            let symbol = action.symbol(t);
            let key = ElementId::Name(format!("{tip}:{id}").into());
            let button: AnyElement = match action {
                RowAction::Star => {
                    let (on, id) = (!t.is_starred(), id.clone());
                    let listener = cx.listener(move |this, _: &ClickEvent, _, cx| this.star_row(&id, on, cx));
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Snooze => {
                    let id = id.clone();
                    let listener = cx.listener(move |this, ev: &ClickEvent, window, cx| this.open_hibernate_menu(id.clone(), below(ev.position()), window, cx));
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Archive => {
                    let id = id.clone();
                    let listener = cx.listener(move |this, _: &ClickEvent, window, cx| {
                        let confirmed_id = id.clone();
                        this.confirm_if_busy(&id, true, window, cx, move |this, window, cx| this.archive_row(confirmed_id.clone(), window, cx));
                    });
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Unsnooze => {
                    let id = id.clone();
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, _, cx| this.wake_row(&id, cx))).into_any_element()
                }
                RowAction::Unarchive => {
                    let id = id.clone();
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, _, cx| this.unarchive_row(&id, cx))).into_any_element()
                }
                RowAction::Delete => {
                    let busy_key = format!("delete:{id}");
                    if self.sidebar.busy.contains(&busy_key) {
                        // On the way: a spinner, and no more clicks.
                        div().flex_none().size(px(22.)).flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(p.secondary).with_size(px(13.))).into_any_element()
                    } else {
                        let id = id.clone();
                        icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, window, cx| this.delete_row(id.clone(), window, cx))).into_any_element()
                    }
                }
            };
            row = row.child(button);
        }
        row.into_any_element()
    }

    /// `SubagentList`: indented rows under the thread with a drawn trunk.
    fn render_subagents(&mut self, t: &ThreadSummary, section: &'static str, now: i64, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // The one whose transcript is open is marked, as a selected row is.
        let selected_sub = self.subagent.as_ref().filter(|(thread, _)| *thread == t.id).map(|(_, id)| id.clone());
        let all = rows::subagents(t);
        if all.is_empty() {
            return vec![];
        }
        let selected = self.selected.as_deref() == Some(t.id.as_str());
        // Under Inbox and Starred only a running one shows.
        let finished_shown = section != "inbox" && section != "starred";
        let shown = rows::subagent_rows(&all, selected, None, now, finished_shown);
        let last = shown.len().saturating_sub(1);
        shown
            .iter()
            .enumerate()
            .map(|(i, row)| render_subagent_row(t, row, i == 0, i == last, selected_sub.as_deref() == Some(row.subagent.id.as_str()), now, p, cx).into_any_element())
            .collect()
    }

    /// The sidebar as `dump` reports it, in the Swift app's shape.
    #[cfg(feature = "automation")]
    pub(crate) fn sidebar_dump(&self, cx: &App) -> Value {
        let now = now_ms();
        let clock: Clock = rows::local_now();
        let selected_sub = self.selected.clone();
        let wakes = threadbear_core::inbox::wakes(&self.scheduled.list);
        let row = |t: &ThreadSummary, section: Option<&str>| -> Value {
            let key = section.unwrap_or("inbox");
            let mode = RowMode::of(key, t, now);
            let look = RowLook::of(t, section);
            let plain = if mode == RowMode::Inbox { look } else { RowLook::PLAIN };
            let tail = rows::trailing(t, &plain, mode, &clock, (key == "hibernating").then(|| wakes.get(&t.id)).flatten());
            let hovered = self.sidebar.hover_override.as_deref() == Some(t.id.as_str());
            let tile = match rows::tile(t).0 {
                Tile::Letter(l) => l,
                Tile::Folder if t.runner_id.is_some() => "runner".to_string(),
                Tile::Folder => "folder".to_string(),
            };
            let mark = if plain.done { Some("tick") } else if plain.unread { Some("dot") } else { None };
            let finished_shown = key != "inbox" && key != "starred";
            let subagents: Vec<Value> = if matches!(key, "hibernating" | "archived") {
                vec![]
            } else {
                let is_selected = selected_sub.as_deref() == Some(t.id.as_str());
                rows::subagent_rows(&rows::subagents(t), is_selected, None, now, finished_shown)
                    .iter()
                    .map(|s| json!({ "id": s.subagent.id, "depth": s.depth, "status": s.subagent.status, "title": rows::subagent_title(&s.subagent), "detail": rows::subagent_detail(&s.subagent), "selected": false }))
                    .collect()
            };
            json!({
                "id": t.id, "title": t.title, "status": t.status, "needsYou": t.needs_you, "unread": look.unread, "hovered": hovered,
                "background": (look.status == rows::RowStatus::Background).then(|| rows::background_short(&t.background_tasks())),
                "waiting": t.waiting_for_room_until_ms().map(|u| rows::waiting_line(u, &clock)).or_else(|| t.waiting_for_network.as_ref().map(|_| "Waiting for connection".to_string())),
                "subagents": subagents,
                "origin": rows::byline(t).map(|b| b.symbol),
                "place": t.place(),
                "bold": plain.emphasised,
                "mark": mark,
                "trailing": tail.text,
                "tile": tile,
                "hoverButtons": hovered.then(|| mode.actions().iter().map(|a| a.tip(t)).collect::<Vec<_>>()),
                "runner": t.runner_id.as_deref().map(|id| self.runner_place_of(id)),
                // Laid out within the part of the list that's showing (null
                // for a row that isn't drawn, as in a folded section).
                "inView": self.sidebar.motion.in_view(&t.id, &self.sidebar.scroll),
            })
        };
        let shown = self.shown_sections(cx, now);
        let searching = !self.search.read(cx).value().trim().is_empty();
        let sections: Vec<Value> = shown
            .iter()
            .map(|s| {
                let folded = self.folded.contains(s.key);
                let count = (!searching && folded && !s.rows.is_empty()).then_some(s.rows.len());
                let drawn = (s.key == "archived").then(|| {
                    let selected = self.selected.as_ref().and_then(|id| s.rows.iter().position(|t| &t.id == id));
                    rows::archive_rows(s.rows.len(), self.sidebar.archive_limit, searching, selected).0
                });
                json!({ "name": s.title, "collapsed": folded, "count": count, "shown": drawn, "threads": s.rows.iter().map(|t| row(t, Some(s.key))).collect::<Vec<_>>() })
            })
            .collect();
        let snoozed = shown.iter().find(|s| s.key == "hibernating").map_or(0, |s| s.rows.len());
        let archived = shown.iter().find(|s| s.key == "archived").map_or(0, |s| s.rows.len());
        json!({ "mode": "inbox", "sections": sections, "snoozed": snoozed, "archived": archived, "sort": self.sidebar.order.name(), "loading": !self.sidebar.loaded })
    }

    /// `row` as `dump` shows the open thread (`selection`).
    #[cfg(feature = "automation")]
    pub(crate) fn selection_dump(&self) -> Value {
        let Some(t) = self.selected.as_ref().and_then(|id| self.threads.get(id)) else { return Value::Null };
        json!({ "id": t.id, "title": t.title, "status": t.status, "needsYou": t.needs_you, "unread": RowLook::of(t, None).unread, "runner": t.runner_id.as_deref().map(|id| self.runner_place_of(id)) })
    }
}

/// `abbreviatingHome`: "/Users/mike/dev/x" becomes "~/dev/x", for the row's tooltip.
fn abbreviate_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path == home => "~".into(),
        Ok(home) if !home.is_empty() && path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// `ProjectTiles`: the row's tile, with the dot or tick in the gutter.
/// `behind` is the colour drawn just under the tile (the row's highlight over
/// the sidebar), which the ring round the front tile takes so a stacked pair
/// reads as Swift's cut-out does. `runner_tile`: another runner's thread with
/// no project draws its machine's icon (a little larger, as a Mac mini is a
/// flat box) in place of the folder, and names it in the tooltip.
fn render_tile(t: &ThreadSummary, look: &RowLook, behind: Hsla, project_icon: Option<&ProjectIcon>, runner_tile: Option<(&'static str, String)>, p: &Palette) -> impl IntoElement {
    let (tile, stacked) = rows::tile(t);
    let help = match &runner_tile {
        Some((_, place)) => format!("{place}, in {}", abbreviate_home(&t.cwd)),
        None => rows::tile_help(t),
    };
    let size = px(TILE);
    let tinted = |content: AnyElement| div().flex().items_center().justify_center().size(size).rounded(px(7.)).bg(p.tile).child(content).into_any_element();
    let face = match (&tile, project_icon) {
        (Tile::Folder, _) => {
            let glyph = match &runner_tile {
                Some((symbol, _)) => icon(symbol, 16., p.secondary),
                None => icon("folder", 12., p.faint),
            };
            div().flex().items_center().justify_center().size(size).rounded(px(7.)).bg(p.tile_empty).child(glyph).into_any_element()
        }
        // The project's picture fills the tile; the letter stands in if the
        // file can't be read (a project on another runner's disk).
        (Tile::Letter(letter), Some(ProjectIcon { image_path: Some(path), .. })) => {
            let (letter, secondary, fill) = (letter.clone(), p.secondary, p.tile);
            img(std::path::PathBuf::from(path))
                .size(size)
                .rounded(px(7.))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || div().flex().items_center().justify_center().size(px(TILE)).rounded(px(7.)).bg(fill).text_size(px(14.)).font_semibold().text_color(secondary).child(letter.clone()).into_any_element())
                .into_any_element()
        }
        // Its symbol, when the bundled set has one for it.
        (Tile::Letter(_), Some(ProjectIcon { symbol: Some(symbol), .. })) if icons::try_symbol(symbol).is_some() => tinted(icons::symbol(symbol).with_size(px(14.)).text_color(p.secondary).into_any_element()),
        (Tile::Letter(letter), _) => tinted(div().text_size(px(14.)).font_semibold().text_color(p.secondary).child(letter.clone()).into_any_element()),
    };
    let mark: Option<AnyElement> = if look.done {
        // Finished with nothing needed: a tick where the dot would be.
        Some(div().absolute().left(px(-(LEADING + 10.) / 2. - 1.)).top_0().bottom_0().flex().items_center().child(icon("checkmark", 10., p.accent)).into_any_element())
    } else if look.unread {
        Some(
            div()
                .absolute()
                .left(px(-(LEADING + 7.) / 2. - 1.))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(div().size(px(7.)).rounded_full().bg(p.accent))
                .into_any_element(),
        )
    } else {
        None
    };
    div()
        .relative()
        .flex_none()
        .size(px(TILE + 4.))
        .id(ElementId::Name(format!("tile-{}", t.id).into()))
        .tip(help.clone())
        .when(stacked, |s| {
            // A second project's edge showing behind, up and to the right.
            s.child(div().absolute().left(px(4.)).top_0().size(size).rounded(px(7.)).bg(p.tile)).child(div().absolute().left_0().bottom_0().size(px(TILE + 2.)).rounded(px(8.)).bg(behind))
        })
        .child(div().absolute().left_0().bottom_0().child(face))
        .when_some(mark, |s, m| s.child(m))
}

/// What the right of the second line says: a word or the time, with its look.
fn render_trailing(trailing: &rows::Trailing, id: String, p: &Palette) -> AnyElement {
    let text = trailing.text.clone();
    let meta = px(style::size::SIDEBAR_META);
    let tip = trailing.tip.clone();
    // A long status ("Waiting for room · 2:51 pm") gives way before the
    // folder and the machine do, as SwiftUI lays out ThreadRowView's line:
    // the shorter text keeps its width and the longer one is cut short.
    // Shrinking twenty times as readily does the same in a flex row.
    let gives_way = |d: Div| {
        let mut d = d.min_w_0().truncate();
        d.style().flex_shrink = Some(20.);
        d
    };
    let el = match trailing.kind {
        TrailingKind::Quiet => gives_way(div()).text_size(meta).line_height(px(15.)).text_color(p.faint).child(text).into_any_element(),
        TrailingKind::Working => h_flex()
            .flex_none()
            .items_baseline()
            .gap(px(5.))
            .text_size(meta).line_height(px(15.))
            .font_medium()
            .text_color(p.accent)
            .child(crate::spinner::TurningArc::new(12., p.accent))
            .child(text)
            .into_any_element(),
        TrailingKind::Background => h_flex()
            .flex_none()
            .items_baseline()
            .gap(px(5.))
            .text_size(meta).line_height(px(15.))
            .font_medium()
            .text_color(p.secondary)
            .child(crate::spinner::TurningArc::new(12., p.secondary))
            .child(text)
            .into_any_element(),
        TrailingKind::Failed => gives_way(div()).text_size(meta).line_height(px(15.)).font_medium().text_color(p.failed).child(text).into_any_element(),
        TrailingKind::Stopped => gives_way(div()).text_size(meta).line_height(px(15.)).text_color(p.secondary).child(text).into_any_element(),
        TrailingKind::Woke => gives_way(div()).text_size(meta).line_height(px(15.)).font_medium().text_color(p.accent).child(text).into_any_element(),
    };
    // A working row's clock and a background one's jobs are short, and keep
    // their width whatever the folder beside them (SwiftUI gives the shorter
    // text its size first): giving way, the clock lost its "s" ("18" for
    // "18s") on a row whose folder filled the line.
    let keeps_width = matches!(trailing.kind, TrailingKind::Working | TrailingKind::Background);
    match tip {
        Some(tip) if keeps_width => div().flex_none().id(ElementId::Name(format!("trailing-{id}").into())).tip(tip.clone()).child(el).into_any_element(),
        Some(tip) => gives_way(div()).id(ElementId::Name(format!("trailing-{id}").into())).tip(tip.clone()).child(el).into_any_element(),
        None => el,
    }
}

/// `LoadingRows`: grey rows the size of thread rows, gently pulsing, until
/// the runner's first list has come.
fn render_loading_rows(p: &Palette) -> impl IntoElement {
    const WIDTHS: [(f32, f32); 6] = [(170., 90.), (130., 70.), (190., 110.), (150., 80.), (120., 95.), (175., 75.)];
    let color = p.skeleton;
    v_flex()
        .pt(px(10.))
        .with_animation("sidebar-skeleton", Animation::new(Duration::from_millis(1800)).repeat().with_easing(bounce(ease_in_out)), |el, delta| el.opacity(1. - delta * 0.5))
        .children(WIDTHS.into_iter().map(move |(title, place)| {
            h_flex()
                .gap(px(10.))
                .pt(px(8.))
                .pb(px(8.))
                .pl(px(LEADING))
                .pr(px(TRAILING))
                .child(div().flex_none().size(px(TILE)).rounded(px(7.)).bg(color))
                .child(v_flex().gap(px(7.)).child(div().w(px(title)).h(px(9.)).rounded_full().bg(color)).child(div().w(px(place)).h(px(7.)).rounded_full().bg(color)))
        }))
}

/// `SubagentRowView`: one indented row, hanging off the thread's tile.
fn render_subagent_row(t: &ThreadSummary, row: &rows::SubagentRow, first: bool, last: bool, selected: bool, now: i64, p: &Palette, cx: &mut Context<Threadbear>) -> impl IntoElement {
    let sub = &row.subagent;
    let elbow_end = TRUNK + 6. + INDENT * row.depth as f32;
    let leading = TRUNK + 9. + INDENT * row.depth as f32;
    let (guide, secondary, faint, fg, failed) = (p.guide, p.secondary, p.faint, p.fg, p.failed);
    let running = sub.running();
    let glyph: AnyElement = match sub.status.as_str() {
        "running" => crate::spinner::TurningArc::new(11., secondary).into_any_element(),
        "completed" => icon("checkmark", 9., faint).into_any_element(),
        "failed" => icon("xmark", 9., failed).into_any_element(),
        _ => icon("minus", 9., faint).into_any_element(),
    };
    let tip = {
        let mut parts = vec![rows::subagent_status_text(sub).to_string()];
        if let Some(model) = rows::subagent_model_text(sub) {
            parts.insert(0, model);
        }
        if let Some(kind) = sub.agent_type.as_deref().filter(|k| !k.is_empty()) {
            parts.insert(0, kind.to_string());
        }
        if let Some(n) = sub.tool_uses.filter(|n| *n > 0) {
            parts.push(if n == 1 { "1 tool call".to_string() } else { format!("{n} tool calls") });
        }
        parts.join(" · ")
    };
    let (thread_id, sub_id) = (t.id.clone(), sub.id.clone());
    let shown_tip: SharedString = format!("{tip}. Click to read its transcript.").into();
    let tooltip_text = shown_tip.clone();
    let hover = p.hover;
    div()
        .id(ElementId::Name(format!("sub-{}-{}", t.id, sub.id).into()))
        .tip(tooltip_text.clone())
        .relative()
        .pt(px(5.))
        .pb(px(6.))
        .pl(px(leading))
        .pr(px(TRAILING))
        .rounded(px(CORNER))
        .when(selected, |s| s.bg(p.selected))
        .hover(move |s| s.bg(hover))
        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| this.open_subagent(thread_id.clone(), sub_id.clone(), window, cx)))
        // A faint line down from the thread's tile with an elbow into the
        // glyph, like a file tree (`SubagentGuide`).
        .child(render_guide(first, last, elbow_end, guide))
        .child(
            h_flex()
                .gap(px(7.))
                .items_start()
                .child(div().flex_none().mt(px(2.)).size(px(11.)).flex().items_center().justify_center().child(glyph))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .items_baseline()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(if running { fg } else { secondary }).child(rows::subagent_title(sub)))
                                .child(div().flex_none().text_size(px(style::size::SIDEBAR_META)).line_height(px(16.)).text_color(faint).child(rows::subagent_duration(sub, now))),
                        )
                        .child(div().truncate().text_size(px(style::size::SIDEBAR_META + 1.)).line_height(px(15.)).text_color(if sub.status == "failed" { failed } else { faint }).child(rows::subagent_detail(sub))),
                ),
        )
}

/// The piece of tree line a sub-agent row draws: down the trunk and across
/// to the glyph (`SubagentGuide`). Drawn as a few plain lines, one pixel wide.
fn render_guide(first: bool, last: bool, elbow_end: f32, color: Hsla) -> impl IntoElement {
    let overhang = if first { 6. } else { 0. };
    let elbow_y = 13.;
    // The last row's trunk stops at its elbow; the others run on down the row.
    let trunk = div().absolute().left(px(TRUNK)).top(px(-overhang)).w(px(1.)).bg(color);
    let trunk = if last { trunk.h(px(elbow_y + overhang + 1.)) } else { trunk.bottom_0() };
    div().absolute().top_0().left_0().right_0().bottom_0().child(trunk).child(div().absolute().left(px(TRUNK)).top(px(elbow_y)).w(px(elbow_end - TRUNK)).h(px(1.)).bg(color))
}

/// Adds `rows` to a gpui-kit menu, with submenus, wiring each line to the
/// action it runs on the app.
fn fill_menu(mut menu: PopupMenu, rows_list: &[MenuRow], entity: WeakEntity<Threadbear>, thread: Option<String>, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    for row in rows_list {
        if row.separator {
            menu = menu.separator();
            continue;
        }
        let symbol = row.symbol.map(icons::symbol);
        if !row.children.is_empty() {
            let (children, entity2, thread2) = (row.children.clone(), entity.clone(), thread.clone());
            menu = menu.submenu_with_icon(symbol, row.title.clone(), window, cx, move |sub, window, cx| fill_menu(sub, &children, entity2.clone(), thread2.clone(), window, cx));
            continue;
        }
        let action = row.action.clone();
        let (entity2, thread2) = (entity.clone(), thread.clone());
        let mut item = PopupMenuItem::new(row.title.clone()).checked(row.checked).disabled(!row.enabled).on_click(move |_, window, cx| {
            if let Some(action) = action.clone() {
                let thread = thread2.clone();
                entity2.update(cx, |this, cx| this.run_menu_action(thread, action, window, cx)).ok();
            }
        });
        if let Some(symbol) = symbol {
            item = item.icon(symbol);
        }
        menu = menu.item(item);
    }
    menu
}

/// Scripted commands for the sidebar, the same names and shapes as the Swift
/// app's (app/README.md): hover, sort, folding a heading, starring and
/// unarchiving, snoozing a row, opening and reading its menus.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    let name = cmd["cmd"].as_str().unwrap_or("");
    // Its full id from the start of it, as the Swift app's `target` does.
    let target = |cx: &App| -> Result<String, String> {
        match cmd["id"].as_str() {
            Some(id) => app.read(cx).thread_id_from(id),
            None => app.read(cx).selected.clone().ok_or_else(|| "no thread open".to_string()),
        }
    };
    match name {
        "hover" => {
            // A schedule's row in the Scheduled view, by its id; else a thread.
            let id = match cmd["id"].as_str() {
                Some(id) if app.read(cx).schedule(id).is_some() => Some(id.to_owned()),
                Some(id) => Some(app.read(cx).thread_id_from(id)?),
                None => None,
            };
            app.update(cx, |t, cx| {
                t.sidebar.hover_override = id.clone();
                cx.notify();
            });
            Ok(json!(id.map_or("ok".to_string(), |id| format!("ok {id}"))))
        }
        // The Archived section's Show more row.
        "archive-more" => {
            app.update(cx, |t, cx| {
                t.sidebar.archive_limit += rows::ARCHIVE_STEP;
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "sort" => {
            let Some(order) = cmd["order"].as_str().and_then(InboxOrder::from_name) else { return Err(format!("order must be one of {:?}", InboxOrder::ALL.map(InboxOrder::name))) };
            app.update(cx, |t, cx| {
                t.sidebar.order = order;
                t.sidebar_settings_changed(cx);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "expand" | "collapse" => {
            // An inbox heading, as clicking it does. The Rust app's section keys
            // are the Swift enum's raw values.
            let section = cmd["section"].as_str().unwrap_or("");
            let key: &'static str = ["starred", "inbox", "running", "hibernating", "archived"].into_iter().find(|k| *k == section).ok_or_else(|| format!("section must be one of starred, inbox, running, hibernating, archived, not {section:?}"))?;
            app.update(cx, |t, cx| {
                if name == "expand" {
                    t.folded.remove(key);
                } else {
                    t.folded.insert(key);
                }
                t.sidebar_settings_changed(cx);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "star" | "unstar" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.star_row(&id, name == "star", cx));
            Ok(json!(format!("ok {id}")))
        }
        "unarchive" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.unarchive_row(&id, cx));
            Ok(json!(format!("ok {id}")))
        }
        "archive" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.archive_row(id.clone(), window, cx));
            Ok(json!(format!("ok {id}")))
        }
        "snooze" => {
            let id = target(cx)?;
            if let Some(said) = cmd["said"].as_str() {
                let said = said.to_string();
                app.update(cx, |t, cx| t.hibernate_when(id.clone(), said, window, cx));
            } else {
                let seconds = cmd["seconds"].as_f64().unwrap_or(3600.);
                let until = now_ms() + (seconds * 1000.) as i64;
                app.update(cx, |t, cx| t.snooze_row(id.clone(), until, window, cx));
            }
            Ok(json!(format!("ok {id}")))
        }
        "new" => {
            app.update(cx, |t, cx| t.new_thread(cx));
            Ok(json!("ok"))
        }
        // A control by its tooltip, through a real click, as the Swift app's
        // `click` with `tip` (or `tipPrefix`, for a tip that ends with a shortcut).
        "click" => {
            let (tip, prefix) = match (cmd["tip"].as_str(), cmd["tipPrefix"].as_str()) {
                (Some(tip), _) => (tip, false),
                (_, Some(tip)) => (tip, true),
                _ => return Err("click needs title, tip or tipPrefix".into()),
            };
            window.render_frame(cx);
            let matches = |label: &str| if prefix { label.starts_with(tip) } else { label == tip };
            let shown = |window: &Window| window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(&matches)).collect::<Vec<_>>();
            let mut found = shown(window);
            // One drawn wholly or partly out of sight in Settings' pane is
            // scrolled to first, as a person would, so the click's middle
            // lands on it (settings_window::scroll_into_view). Partly counts
            // as shown above, with its middle maybe past the pane's edge.
            if cmd["settings"].as_bool() == Some(true) {
                let target = found.last().cloned().or_else(|| window.elements().into_iter().filter(|e| e.label().is_some_and(&matches)).last());
                if target.is_some_and(|t| crate::settings_window::scroll_into_view(app, t.bounds(), cx)) {
                    window.refresh();
                    window.render_frame(cx);
                    found = shown(window);
                }
            }
            let Some(target) = found.last() else { return Err(format!("no control with the tip {tip:?}")) };
            let at = target.bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Left, cx);
            window.pointer_up(at, MouseButton::Left, cx);
            Ok(json!("ok"))
        }
        // A row's right-click menu or a button's menu, opened the way a click
        // does and read back; `tipPrefix` names the control by the start of
        // its tip. As the Swift app's: `right` right-clicks (a left click otherwise,
        // the sort button's) and `ticks` puts "[x] " before a ticked line.
        "realMenu" => {
            let prefix = cmd["tipPrefix"].as_str().ok_or("realMenu here takes tipPrefix")?;
            let button = if cmd["right"].as_bool() == Some(true) { MouseButton::Right } else { MouseButton::Left };
            // A click through real events and hit testing, as a person's
            // goes, on the control whose tooltip starts with this.
            window.render_frame(cx);
            let Some(row) = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| l.starts_with(prefix))).last() else {
                return Err(format!("no row with the tip {prefix:?} on screen"));
            };
            let at = row.bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, button, cx);
            window.pointer_up(at, button, cx);
            let ticks = cmd["ticks"].as_bool() == Some(true);
            let titles: Vec<String> = app
                .read(cx)
                .sidebar
                .menu
                .iter()
                .flat_map(|m| m.rows.iter())
                .filter(|r| !r.separator)
                .map(|r| format!("{}{}{}", if ticks && r.checked { "[x] " } else { "" }, r.title, if r.children.is_empty() { "" } else { " >" }))
                .collect();
            if titles.is_empty() {
                return Err("no menu opened".into());
            }
            app.update(cx, |t, cx| {
                t.sidebar.menu = None;
                cx.notify();
            });
            Ok(json!(format!("ok {}", serde_json::to_string(&titles).unwrap_or_default())))
        }
        // The menus that stand in for a real menu in the Swift app: `show`
        // opens the Hibernate menu for a thread, `click` picks a line by
        // its title, and with neither it lists what's on show.
        "menu-popover" => {
            if let Some(title) = cmd["click"].as_str() {
                let (thread, action) = {
                    let t = app.read(cx);
                    let m = t.sidebar.menu.as_ref().ok_or_else(|| format!("no menu showing to pick {title:?}"))?;
                    let row = m.rows.iter().find(|r| r.title == title).ok_or_else(|| format!("no row {title}"))?;
                    (m.thread.clone(), row.action.clone().ok_or_else(|| format!("{title} has no action"))?)
                };
                app.update(cx, |t, cx| t.run_menu_action(thread, action, window, cx));
                return Ok(json!("ok"));
            }
            if let Some(which) = cmd["show"].as_str() {
                if which != "hibernate" {
                    return Err(format!("menu-popover show {which:?} isn't drawn here yet"));
                }
                let id = target(cx)?;
                app.update(cx, |t, cx| t.open_hibernate_menu(id, point(px(320.), px(160.)), window, cx));
                return Ok(json!("ok"));
            }
            let t = app.read(cx);
            let lines: Vec<String> = t
                .sidebar
                .menu
                .iter()
                .flat_map(|m| m.rows.iter())
                .map(|r| if r.separator { "---".to_string() } else { format!("{}{}", if r.checked { "[x] " } else { "" }, r.title) })
                .collect();
            let shown = t.sidebar.menu.is_some();
            Ok(json!(format!("ok shown {shown} {}", serde_json::to_string(&lines).unwrap_or_default())))
        }
        _ => Err(format!("unknown sidebar command {name}")),
    }
}
