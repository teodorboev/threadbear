// The window's shell: BearDock's panels with the activity bar down the left and
// the rail of the thread's panels down the right (RootView in WindowDock.swift),
// the title bar handed over to the panels' bars, and the app's commands, their
// keys, the menu bar and back and forward (ThreadbearApp.swift, AppShortcuts,
// Navigation.swift).
use crate::app::Threadbear;
use crate::icons;
use crate::settings::WindowFrame;
use crate::style::{self, px};
use gpui_kit::component::{v_flex, ActiveTheme as _, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use threadbear_core::shortcuts::{ShortcutBindings, ShortcutCommand};
use threadbear_core::zoom;
use threadbear_dock::{DockContent, DockDrop, DockEdge, DockGroup, DockLayout, DockNode, DockOptions, DockSplit, SplitAxis};

// The panels' ids, the same as the Swift app's so a saved layout carries over
// (WindowDock, PaneSection, BuiltinPanels, AgentBrowser.panelId).
pub(crate) const THREADS: &str = "threadbear.threads";
pub(crate) const MAIN: &str = "threadbear.thread";
pub(crate) const HIGHLIGHTS: &str = "threadbear.highlights";
pub(crate) const FILES: &str = "threadbear.files";
pub(crate) const PATCHES: &str = "threadbear.patches";
pub(crate) const BROWSER: &str = crate::agent_browser::BROWSER;
pub(crate) const USAGE: &str = "threadbear.usage";
pub(crate) const TERMINAL: &str = "threadbear.terminal";
/// The activity bar's other panels (activity_panels.rs), open until Mike
/// closes them.
pub(crate) const UPDATES: &str = "threadbear.updates";
pub(crate) const CONNECTORS: &str = "threadbear.connectors";
pub(crate) const RUNNERS: &str = "threadbear.runners";
/// Finish setting up (setup_panel.rs), open until Mike closes it.
pub(crate) const SETUP: &str = "threadbear.setup";

/// How often the settings file is written during a burst of changes.
const SAVE_THROTTLE_MS: i64 = 300;

/// `SidebarMode`: which view the sidebar shows, from the activity bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SidebarMode {
    #[default]
    Inbox,
    Scheduled,
    Projects,
    Librarian,
}

impl SidebarMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            SidebarMode::Inbox => "inbox",
            SidebarMode::Scheduled => "scheduled",
            SidebarMode::Projects => "projects",
            SidebarMode::Librarian => "librarian",
        }
    }
}

/// `NavigationPlace`: where the window is, for back and forward. The sub-agent
/// view comes with that feature.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NavPlace {
    pub(crate) mode: SidebarMode,
    pub(crate) thread: Option<String>,
    /// The mode's page was in the main panel (`scheduledHome` etc.).
    pub(crate) home: bool,
    /// A project's page was (`projectId`).
    pub(crate) project: Option<String>,
}

/// "Archived" and the thread's name, floating over the main panel with Undo
/// (`ArchivedNote`).
#[derive(Clone, Debug)]
pub(crate) struct ArchivedNote {
    pub(crate) id: u64,
    pub(crate) thread: String,
    pub(crate) title: String,
    pub(crate) was_open: bool,
}

/// Next Inbox Thread with nowhere to go (`InboxEndNote`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct InboxEndNote {
    pub(crate) id: u64,
    pub(crate) empty: bool,
}

/// Swift's `WindowDock.defaultLayout`: the sidebar without a title, the
/// thread, and Highlights, side by side.
pub(crate) fn default_layout() -> DockLayout {
    let group = |id: &str, panel: &str| DockNode::Group(DockGroup::new(id, vec![panel.to_string()], None));
    let root = DockNode::Split(DockSplit::new("window", SplitAxis::Horizontal, vec![group("sidebar", THREADS), group("main", MAIN), group("pane", HIGHLIGHTS)], Some(vec![0.29, 0.47, 0.24])));
    DockLayout::new(root, [THREADS.to_string()], vec![])
}

/// The saved layout, or the default; without the main panel there's nowhere
/// to type, so one saved that way gets it back (`makeController`).
pub(crate) fn make_layout(saved: Option<&Value>) -> DockLayout {
    let mut layout = saved.and_then(|v| serde_json::from_value::<DockLayout>(v.clone()).ok()).unwrap_or_else(default_layout);
    if !layout.contains(MAIN) {
        layout.add(MAIN, DockDrop::window(DockEdge::Right));
    }
    layout
}

/// The `DockOptions` for this window, as `RootView.options` makes them: the
/// bars are the window's title bar, with room for the traffic lights in the
/// top-left panel's bar (the activity bar has the first 44 points of them).
/// `settings` is the app's, for the switches in Settings that change the dock.
pub(crate) fn dock_options(app: WeakEntity<Threadbear>, settings: &crate::settings::Settings, cx: &App) -> DockOptions {
    let traffic_lights = if cfg!(target_os = "macos") { (78. - f32::from(px(crate::rails::BAR_WIDTH))).max(0.) } else { 0. };
    DockOptions {
        header_height: f32::from(px(crate::header::HEADER_HEIGHT)),
        title_size: f32::from(px(style::size::LABEL)),
        traffic_lights,
        corner_moves_window: true,
        bars_move_window: true,
        background_moves_panel: settings.panel_background_drag(),
        accent: Some(style::control_accent(cx)),
        window_content: None,
        prepare_window: None,
        // A panel's window of its own in front counts as Mike looking, as
        // the main window does (`Threadbear::looking_changed`). Going, it's
        // only noted: that can come in the middle of the app's own update.
        window_activation: Some(std::rc::Rc::new(move |active: bool, window: &mut Window, cx: &mut App| {
            // A script's click in that window isn't Mike looking.
            if active && crate::thread::input_is_scripted() {
                return;
            }
            let key = crate::thread::window_key(window);
            match app.upgrade() {
                Some(app) if active => app.update(cx, |app, cx| app.looking_changed(key, true, cx)),
                _ => crate::thread::set_looking(key, active),
            }
        })),
    }
}

/// What each panel shows and is called: the seam BearDock draws through
/// (Swift's `DockContent` in `WindowDock`).
pub(crate) struct Panels {
    pub(crate) app: WeakEntity<Threadbear>,
}

impl DockContent for Panels {
    fn title(&self, panel: &str, cx: &App) -> SharedString {
        self.app.upgrade().map(|a| SharedString::from(a.read(cx).panel_title(panel))).unwrap_or_default()
    }

    fn icon(&self, panel: &str, _window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let color = cx.theme().muted_foreground;
        if let Some(symbol) = panel_symbol(panel) {
            return Some(icons::symbol(symbol).with_size(px(13.)).text_color(color).into_any_element());
        }
        // A patch's panel: its own picture or symbol, as on the rail.
        let (found, thread) = self.patch_panel(panel, cx)?;
        let symbol = threadbear_core::highlights::panel_symbol(&found).to_owned();
        let icon = crate::patch_icons::patch_icon(found.icon_file.as_deref(), &symbol, 13., Some(color), thread.is_remote());
        // A pinned one has a pin before it (pinned_panels.rs).
        Some(if crate::pinned_panels::pinned(panel).is_some() { crate::pinned_panels::tab_icon(icon, cx) } else { icon })
    }

    fn badge(&self, panel: &str, _window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        // Where a patch's panel came from, over its icon's corner, smaller
        // than the rail's (`PanelOriginBadge(side: 11)` on a tab).
        let (found, thread) = self.patch_panel(panel, cx)?;
        let origin = threadbear_core::highlights::PanelOrigin::of_panel(&found, &thread.patches());
        let icons = self.app.upgrade()?.read(cx).sidebar.project_icons.clone();
        crate::patch_icons::origin_badge(&origin, crate::patch_icons::project_mark(&origin, &thread, &icons), 10., cx)
    }

    fn view(&self, panel: &str, ghost: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.panel_view(panel, ghost, window, cx)),
            None => div().into_any_element(),
        }
    }

    fn header(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let app = self.app.upgrade()?;
        // A thread in a panel of its own has its title line too (thread_panels.rs).
        if crate::thread_panels::pinned_thread(panel).is_some() {
            return app.update(cx, |t, cx| t.thread_panel_header(panel, cx));
        }
        if panel != MAIN {
            return None;
        }
        Some(app.update(cx, |t, cx| t.render_header_title(window, cx)))
    }

    fn accessory(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let app = self.app.upgrade()?;
        // A thread in a panel of its own: its Share and ... menu (thread_panels.rs).
        if crate::thread_panels::pinned_thread(panel).is_some() {
            return app.update(cx, |t, cx| t.thread_panel_buttons(panel, cx));
        }
        if panel != MAIN {
            // A web panel's Back, Reload and Open in browser (web_panels.rs).
            return app.update(cx, |t, cx| crate::web_panels::bar_buttons(t, panel, cx));
        }
        Some(app.update(cx, |t, cx| t.render_header_buttons(window, cx)))
    }

    fn closable(&self, panel: &str, _cx: &App) -> bool {
        panel != MAIN
    }

    fn available(&self, panel: &str, cx: &App) -> bool {
        let Some(app) = self.app.upgrade() else { return true };
        app.read(cx).panel_available(panel, cx)
    }

    /// Pin on a thread's patch panel, Unpin on a pinned one (pinned_panels.rs).
    fn menu_items(&self, panel: &str, cx: &App) -> Vec<threadbear_dock::DockMenuItem> {
        let Some(app) = self.app.upgrade() else { return vec![] };
        let Some(label) = app.read(cx).pin_menu_label(panel) else { return vec![] };
        let icon = if label == "Unpin" { gpui_kit::assets::IconName::PinOff } else { gpui_kit::assets::IconName::Pin };
        let (weak, panel) = (self.app.clone(), panel.to_owned());
        vec![threadbear_dock::DockMenuItem {
            label: label.into(),
            icon: Some(icon),
            on_click: std::rc::Rc::new(move |_, cx| {
                weak.update(cx, |t, cx| t.choose_pin_menu(&panel, cx)).ok();
            }),
        }]
    }

    /// A pinned panel's tab says whose it is.
    fn tip(&self, panel: &str, cx: &App) -> Option<SharedString> {
        self.app.upgrade()?.read(cx).pinned_tip(panel).map(SharedString::from)
    }
}

/// `WindowDock.symbol(of:)`: the icon on a panel's tab.
impl Panels {
    /// The open thread's patch panel with this id, and the thread.
    fn patch_panel(&self, panel: &str, cx: &App) -> Option<(threadbear_core::highlights::Panel, threadbear_core::inbox::ThreadSummary)> {
        let app = self.app.upgrade()?;
        // A pinned panel's is its own thread's (pinned_panels.rs).
        if crate::pinned_panels::pinned(panel).is_some() {
            let (thread, found) = app.read(cx).pinned_owner(panel)?;
            return Some((found, thread));
        }
        let thread = app.read(cx).pane_thread()?;
        let found = thread.panels().into_iter().find(|p| p.id == panel)?;
        Some((found, thread))
    }
}

fn panel_symbol(panel: &str) -> Option<&'static str> {
    Some(match panel {
        THREADS => "bubble.left.and.bubble.right",
        MAIN => "text.alignleft",
        USAGE => "gauge.with.dots.needle.33percent",
        UPDATES => "arrow.down.circle",
        CONNECTORS => "powerplug",
        RUNNERS => "server.rack",
        SETUP => "checklist",
        TERMINAL => "terminal",
        HIGHLIGHTS => "pin",
        FILES => "folder",
        BROWSER => "globe",
        PATCHES => "square.dashed",
        // A thread in a panel of its own, as the main panel's.
        _ if crate::thread_panels::pinned_thread(panel).is_some() => "text.alignleft",
        _ => return None,
    })
}

// The commands: one per menu item or key, each with the action GPUI dispatches.
// The macro keeps the action, the Swift ShortcutCommand and the handler in one
// line per command.
actions!(
    threadbear,
    [
        NewThread,
        ArchiveThread,
        PickHibernateTime,
        WakeWhenPrompt,
        StopTurn,
        CopyTranscript,
        GoBack,
        GoForward,
        NextInboxThread,
        OpenAnything,
        OpenAnythingAlso,
        ShowInbox,
        ShowScheduled,
        ShowProjects,
        ShowLibrarian,
        ShowHibernating,
        ShowArchived,
        ShowPreviousTab,
        ShowNextTab,
        OpenSettings,
        ZoomIn,
        ZoomOut,
        ActualSize,
        ToggleThreads,
        TogglePanels,
        ToggleTerminal,
        ResetPanelLayout,
        CollapseThreads,
        ToggleFrameRate,
        AboutApp,
        RestartToUpdate,
        TakeTheTour,
        FinishSettingUp,
        HibernateHour,
        HibernateEvening,
        HibernateTomorrow,
        HibernateMonday,
        HibernateRestart,
        QuitApp,
        HideApp,
        HideOtherApps,
        ShowAllApps,
        MinimizeWindow,
        ZoomWindow,
        BringAllToFront,
        CloseWindow,
    ]
);

macro_rules! commands {
    ($(($variant:ident, $action:ident, $shortcut:expr)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum Command { $($variant),* }

        impl Command {
            pub(crate) const ALL: &'static [Command] = &[$(Command::$variant),*];

            /// The action GPUI dispatches for this command, for the menu bar.
            fn action(self) -> Box<dyn Action> {
                match self { $(Command::$variant => Box::new($action)),* }
            }

            fn binding(self, chord: &str) -> KeyBinding {
                let chord = leak(chord);
                // No context: a binding with no context wins at any depth, so
                // these keys always do their menu command (as a Mac app's
                // menu key equivalents do), even in the composer, where a
                // text field's own binding for ⌘[ or ⌥⌘↓ would otherwise take
                // the key first.
                match self { $(Command::$variant => KeyBinding::new(chord, $action, None)),* }
            }

            /// The shortcut this command's key comes from, if it has one.
            pub(crate) fn shortcut(self) -> Option<ShortcutCommand> {
                match self { $(Command::$variant => $shortcut),* }
            }
        }

        fn register_commands(root: Stateful<Div>, cx: &mut Context<Threadbear>) -> Stateful<Div> {
            // From the menu bar or a key: noted in the activity log as the
            // menu item it is (activity.rs), then run.
            root $(.on_action(cx.listener(|this, _: &$action, window, cx| this.command_chosen(Command::$variant, window, cx))))*
        }
    };
}

commands!(
    (NewThread, NewThread, Some(ShortcutCommand::NewThread)),
    (Archive, ArchiveThread, Some(ShortcutCommand::Archive)),
    (Hibernate, PickHibernateTime, Some(ShortcutCommand::Hibernate)),
    (WakeWhen, WakeWhenPrompt, None),
    (Stop, StopTurn, Some(ShortcutCommand::Stop)),
    (CopyTranscript, CopyTranscript, Some(ShortcutCommand::CopyTranscript)),
    (Back, GoBack, Some(ShortcutCommand::Back)),
    (Forward, GoForward, Some(ShortcutCommand::Forward)),
    (NextInboxThread, NextInboxThread, Some(ShortcutCommand::NextInboxThread)),
    (OpenAnything, OpenAnything, Some(ShortcutCommand::OpenAnything)),
    // ⌘P: the same box from a second key, with no menu item of its own.
    (OpenAnythingAlso, OpenAnythingAlso, Some(ShortcutCommand::OpenAnythingAlso)),
    (ShowInbox, ShowInbox, Some(ShortcutCommand::ShowInbox)),
    (ShowScheduled, ShowScheduled, Some(ShortcutCommand::ShowScheduled)),
    (ShowProjects, ShowProjects, Some(ShortcutCommand::ShowProjects)),
    (ShowLibrarian, ShowLibrarian, Some(ShortcutCommand::ShowLibrarian)),
    (ShowHibernating, ShowHibernating, Some(ShortcutCommand::ShowHibernating)),
    (ShowArchived, ShowArchived, Some(ShortcutCommand::ShowArchived)),
    (PreviousTab, ShowPreviousTab, Some(ShortcutCommand::PreviousTab)),
    (NextTab, ShowNextTab, Some(ShortcutCommand::NextTab)),
    (Settings, OpenSettings, Some(ShortcutCommand::Settings)),
    (ZoomIn, ZoomIn, Some(ShortcutCommand::ZoomIn)),
    (ZoomOut, ZoomOut, Some(ShortcutCommand::ZoomOut)),
    (ActualSize, ActualSize, Some(ShortcutCommand::ActualSize)),
    (ToggleThreads, ToggleThreads, Some(ShortcutCommand::ToggleThreads)),
    (TogglePanels, TogglePanels, Some(ShortcutCommand::TogglePanels)),
    (ToggleTerminal, ToggleTerminal, Some(ShortcutCommand::ToggleTerminal)),
    (ResetPanelLayout, ResetPanelLayout, Some(ShortcutCommand::ResetPanelLayout)),
    (CollapseThreads, CollapseThreads, Some(ShortcutCommand::CollapseThreads)),
    (ToggleFrameRate, ToggleFrameRate, None),
    (About, AboutApp, None),
    (RestartToUpdate, RestartToUpdate, None),
    (TakeTheTour, TakeTheTour, None),
    (FinishSettingUp, FinishSettingUp, None),
    (HibernateHour, HibernateHour, None),
    (HibernateEvening, HibernateEvening, None),
    (HibernateTomorrow, HibernateTomorrow, None),
    (HibernateMonday, HibernateMonday, None),
    (HibernateRestart, HibernateRestart, None),
    (Quit, QuitApp, None),
    (Hide, HideApp, None),
    (HideOthers, HideOtherApps, None),
    (ShowAll, ShowAllApps, None),
    (Minimize, MinimizeWindow, None),
    (Zoom, ZoomWindow, None),
    (BringAllToFront, BringAllToFront, None),
    (Close, CloseWindow, None),
);

/// Every Mac app's own keys (the standard app, File and Window menus),
/// whatever shortcuts.json says: ⌘Q quits, ⌘H hides, ⌥⌘H hides the rest,
/// ⌘W closes the window in front, ⌘M minimizes. Elsewhere Control-Q quits,
/// as apps there do.
/// How the menu shows a standard key (`standard_keys`).
fn standard_key_display(command: Command) -> Option<&'static str> {
    let mac = cfg!(target_os = "macos");
    match command {
        Command::Quit => Some(if mac { "⌘Q" } else { "Ctrl+Q" }),
        Command::Hide if mac => Some("⌘H"),
        Command::HideOthers if mac => Some("⌥⌘H"),
        Command::Close if mac => Some("⌘W"),
        Command::Minimize if mac => Some("⌘M"),
        _ => None,
    }
}

fn standard_keys() -> Vec<(&'static str, Command)> {
    if cfg!(target_os = "macos") {
        vec![("cmd-q", Command::Quit), ("cmd-h", Command::Hide), ("alt-cmd-h", Command::HideOthers), ("cmd-w", Command::Close), ("cmd-m", Command::Minimize)]
    } else {
        vec![("ctrl-q", Command::Quit)]
    }
}

impl Threadbear {
    pub(crate) fn panel_title(&self, panel: &str) -> String {
        if let Some(title) = self.thread_panel_title(panel).or_else(|| self.pinned_title(panel)) {
            return title;
        }
        match panel {
            THREADS => "Threads".into(),
            MAIN => self.main_panel_title(),
            USAGE => "Usage".into(),
            UPDATES => "Updates".into(),
            CONNECTORS => "Connectors".into(),
            RUNNERS => "Runners".into(),
            SETUP => "Finish setting up".into(),
            TERMINAL => "Terminal".into(),
            HIGHLIGHTS => "Highlights".into(),
            FILES => "Files".into(),
            BROWSER => "Browser".into(),
            PATCHES => "Patches".into(),
            _ => self.pane_thread().and_then(|t| t.panels.iter().flatten().find(|p| p["id"].as_str() == Some(panel)).and_then(|p| p["title"].as_str()).map(String::from)).unwrap_or_else(|| panel.to_string()),
        }
    }

    /// `WindowDock.title(of: .main)` and the window's own title.
    pub(crate) fn main_panel_title(&self) -> String {
        if let Some(project) = self.open_project() {
            return project.title.clone();
        }
        match self.home_page().unwrap_or(SidebarMode::Inbox) {
            SidebarMode::Projects => "Projects".into(),
            SidebarMode::Librarian => "Librarian".into(),
            SidebarMode::Scheduled => "Scheduled".into(),
            SidebarMode::Inbox => match self.open_thread() {
                Some(t) if t.title.is_empty() => "Untitled".into(),
                Some(t) => t.title.clone(),
                None => "New thread".into(),
            },
        }
    }

    pub(crate) fn open_thread(&self) -> Option<&threadbear_core::inbox::ThreadSummary> {
        self.selected.as_ref().and_then(|id| self.threads.get(id)).map(|t| &**t)
    }

    /// `WindowDock.section(_:)` != nil: a pane section's panel, not the
    /// sidebar, the main panel, one of the activity bar's panels or a thread
    /// in a panel of its own.
    pub(crate) fn is_pane_section(panel: &str) -> bool {
        !matches!(panel, THREADS | MAIN | USAGE | UPDATES | CONNECTORS | RUNNERS | SETUP | TERMINAL) && crate::thread_panels::pinned_thread(panel).is_none() && crate::pinned_panels::pinned(panel).is_none()
    }

    /// `available` in `RootView.content`: Highlights, Files and a thread's own
    /// panels for a thread that has them; the rest always.
    pub(crate) fn panel_available(&self, panel: &str, cx: &App) -> bool {
        // Folded away with ⌘/: the main panel takes its room, and it comes
        // back at the same width (thread_panels.rs).
        if panel == THREADS {
            return !self.threads_collapsed();
        }
        // A thread's own panel while there's such a thread: one deleted on
        // another machine leaves it nothing to show.
        if let Some(id) = crate::thread_panels::pinned_thread(panel) {
            return self.threads.contains_key(id);
        }
        // A pinned panel while its thread has the panel, whichever is picked
        // (pinned_panels.rs).
        if crate::pinned_panels::pinned(panel).is_some() {
            return self.pinned_owner(panel).is_some();
        }
        if !Self::is_pane_section(panel) {
            return true;
        }
        // A project's page has no Highlights or Files (they're a thread's),
        // and only the panels its folders give (`pane_thread`).
        let project_page = self.projects.open.is_some();
        let Some(thread) = self.pane_thread() else { return !project_page };
        match panel {
            HIGHLIGHTS | FILES => !project_page,
            // While BrowserBear is on this Mac, as the rail's button.
            BROWSER => !project_page && self.browser_rail(&thread.id, cx).is_some(),
            PATCHES => thread.patches.as_ref().is_some_and(|p| !p.is_empty()),
            other => thread.panels.iter().flatten().any(|p| p["id"].as_str() == Some(other)),
        }
    }

    fn is_panel_showing(&self, panel: &str, cx: &App) -> bool {
        self.dock.read(cx).controller.is_showing(panel)
    }

    /// The pane sections open in the dock, in layout order (`openPanes`),
    /// and pinned panels, which sit among them.
    fn open_panes(&self, cx: &App) -> Vec<String> {
        self.dock.read(cx).layout().panels().into_iter().filter(|p| Self::is_pane_section(p) || crate::pinned_panels::pinned(p).is_some()).collect()
    }

    /// Shows a section's panel, beside another pane section if one's open,
    /// else down the right of the window (`showPane`).
    pub(crate) fn show_pane(&mut self, panel: &str, cx: &mut Context<Self>) {
        let near = self.open_panes(cx);
        let near_refs: Vec<&str> = near.iter().map(String::as_str).collect();
        self.dock.update(cx, |m, cx| m.show(panel, &near_refs, DockDrop::window(DockEdge::Right), cx));
    }

    /// A rail button, VS Code's way: closes the section's panel if it's
    /// showing, else brings it up (`clickRail`).
    pub(crate) fn click_rail(&mut self, panel: &str, cx: &mut Context<Self>) {
        // The thread's own panel is pinned: that's it, brought to the front
        // rather than a second copy opened. Unpin is how it goes.
        if let Some(pin) = self.own_pin(panel, cx).filter(|_| !self.is_panel_showing(panel, cx)) {
            self.dock.update(cx, |m, cx| m.select(&pin, cx));
            return;
        }
        if self.is_panel_showing(panel, cx) {
            self.dock.update(cx, |m, cx| m.close(panel, cx));
        } else {
            self.show_pane(panel, cx);
        }
    }

    /// One of the activity bar's panels (Usage, Updates, Connectors, Runners):
    /// as a tab beside another of them if one's open, else under the threads
    /// list (`setShownUnderThreads`).
    pub(crate) fn toggle_under_threads(&mut self, panel: &str, cx: &mut Context<Self>) {
        if self.is_panel_showing(panel, cx) {
            self.dock.update(cx, |m, cx| m.close(panel, cx));
            return;
        }
        let under = self.dock.read(cx).layout().group_containing(THREADS).map(|g| DockDrop::edge(g.id, DockEdge::Bottom)).unwrap_or(DockDrop::window(DockEdge::Left));
        let near: Vec<&str> = [USAGE, UPDATES, SETUP, CONNECTORS, RUNNERS].into_iter().filter(|p| *p != panel).collect();
        self.dock.update(cx, |m, cx| m.show(panel, &near, under, cx));
    }

    /// Hide or Show Threads (`toggleThreadsPanel`): closed, or opened down
    /// the left of the window; folded away with ⌘/ counts as hidden.
    fn toggle_threads_panel(&mut self, cx: &mut Context<Self>) {
        let contains = self.dock.read(cx).layout().contains(THREADS);
        if contains && !self.threads_collapsed() {
            self.dock.update(cx, |m, cx| m.close(THREADS, cx));
            return;
        }
        self.set_threads_collapsed(false, cx);
        if !contains {
            self.dock.update(cx, |m, cx| m.open(THREADS, DockDrop::window(DockEdge::Left), cx));
        }
    }

    /// `panelsShown.toggle()`: the pane sections go away, remembered, and
    /// come back as they were (Highlights if none were open).
    fn toggle_panels(&mut self, cx: &mut Context<Self>) {
        let open = self.open_panes(cx);
        if open.is_empty() {
            let panes = if self.hidden_panes.is_empty() { vec![HIGHLIGHTS.to_string()] } else { std::mem::take(&mut self.hidden_panes) };
            for panel in &panes {
                self.show_pane(panel, cx);
            }
        } else {
            self.dock.update(cx, |m, cx| {
                for panel in &open {
                    m.close(panel, cx);
                }
            });
            self.hidden_panes = open;
        }
    }

    pub(crate) fn reset_panel_layout(&mut self, cx: &mut Context<Self>) {
        self.dock.update(cx, |m, cx| m.set_layout(default_layout(), cx));
    }

    /// Saves the panel layout (`dock.onChange`), a moment after it last moved:
    /// a divider drag changes it every frame.
    pub(crate) fn dock_layout_changed(&mut self, layout: DockLayout, cx: &mut Context<Self>) {
        crate::activity::panels(crate::activity::showing(&layout));
        self.settings.saved.dock_layout = serde_json::to_value(&layout).ok();
        self.save_settings_soon(cx);
        // Threads' own panels and the main panel's tabs (thread_panels.rs).
        self.thread_panels_layout_changed(&layout, cx);
        // A patch panel come into sight goes in ⌘K's Recent.
        self.note_panels_shown(&layout, cx);
        self.pinned_panels_layout_changed(&layout);
        cx.notify();
    }

    /// Writes the settings file now, or just after a burst of changes (a
    /// divider drag changes the layout every frame): a change on its own is
    /// on disk straight away, so nothing's lost if the app is killed next.
    pub(crate) fn save_settings_soon(&mut self, cx: &mut Context<Self>) {
        let now = crate::now_ms();
        if self.settings_save_task.is_none() && now - self.settings_last_save_ms >= SAVE_THROTTLE_MS {
            self.settings_last_save_ms = now;
            self.settings_dirty = false;
            self.settings.save();
            return;
        }
        self.settings_dirty = true;
        if self.settings_save_task.is_some() {
            return;
        }
        self.settings_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(SAVE_THROTTLE_MS as u64)).await;
            this.update(cx, |this, _| {
                this.settings_save_task = None;
                this.settings_last_save_ms = crate::now_ms();
                if std::mem::take(&mut this.settings_dirty) {
                    this.settings.save();
                }
            })
            .ok();
        }));
    }

    /// The window's frame, whenever it moves or resizes, unless this is a
    /// scripted run: those frames aren't Mike's (`MainWindowFrame.scripted`).
    pub(crate) fn window_frame_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::env::var_os("THREADBEAR_AUTOMATION").is_some() {
            return;
        }
        let WindowBounds::Windowed(b) = window.window_bounds() else { return };
        self.settings.saved.window_frame = Some(WindowFrame { x: f64::from(b.origin.x), y: f64::from(b.origin.y), width: f64::from(b.size.width), height: f64::from(b.size.height) });
        self.save_settings_soon(cx);
    }

    /// Folds and the Inbox's sort, saved as they change.
    pub(crate) fn sidebar_settings_changed(&mut self, cx: &mut Context<Self>) {
        self.settings.saved.collapsed_sections = Some(self.folded.iter().map(|k| k.to_string()).collect());
        self.settings.set_inbox_order(self.sidebar.order);
        self.save_settings_soon(cx);
    }

    /// Gives the dock its options again, after the zoom or a setting it
    /// reads changed.
    fn refresh_dock_options(&mut self, cx: &mut Context<Self>) {
        let options = dock_options(cx.entity().downgrade(), &self.settings, cx);
        self.dock_view.update(cx, |d, cx| d.set_options(options, cx));
    }

    /// Settings > General's Move panels by their background.
    pub(crate) fn set_panel_background_drag(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.saved.panel_background_drag = Some(on);
        self.save_settings_soon(cx);
        self.refresh_dock_options(cx);
    }

    /// Zoom In, Zoom Out, Actual Size (TextZoom): the zoom goes in `style`, so
    /// every `px` in the window scales; the panels' bars and the transcript's
    /// measured rows follow, and it's saved (and `textZoomPercent` is the
    /// setting's name so a Swift import carries it).
    pub(crate) fn set_zoom(&mut self, percent: u32, window: &mut Window, cx: &mut Context<Self>) {
        let percent = zoom::sanitize(Some(percent));
        if percent == style::zoom_percent() {
            return;
        }
        style::set_zoom_percent(percent);
        apply_zoom_to_theme(percent, cx);
        self.refresh_dock_options(cx);
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.remeasure_rows(cx));
        }
        self.settings.set_zoom_percent(percent);
        self.save_settings_soon(cx);
        window.refresh();
        cx.notify();
    }

    fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(zoom::zoom_in(style::zoom_percent()), window, cx);
    }

    fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(zoom::zoom_out(style::zoom_percent()), window, cx);
    }

    // Commands.

    pub(crate) fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewThread => self.new_thread(cx),
            Command::Archive => self.toggle_archive(window, cx),
            Command::Hibernate => {
                if let Some(id) = self.selected.clone() {
                    self.open_pick_time(id, window, cx);
                }
            }
            Command::WakeWhen => {
                if let Some(id) = self.selected.clone() {
                    self.open_wake_when(id, window, cx);
                }
            }
            Command::Stop => self.stop_open(cx),
            Command::CopyTranscript => {
                if let Some(pane) = &self.pane {
                    pane.update(cx, |p, cx| p.copy_transcript(cx));
                }
            }
            Command::Back => self.go_back(window, cx),
            Command::Forward => self.go_forward(window, cx),
            Command::NextInboxThread => self.next_inbox_thread(window, cx),
            // A project's page tabs; they do nothing anywhere else.
            Command::PreviousTab => self.step_project_tab(-1, cx),
            Command::NextTab => self.step_project_tab(1, cx),
            // Only while the experiment's on: its keys and menu item go
            // with it, but a stale binding mustn't open it either.
            Command::OpenAnything | Command::OpenAnythingAlso => {
                if self.open_anything_on() {
                    self.toggle_open_anything(window, cx);
                }
            }
            // Each brings the Threads panel back if ⌘/ folded it away.
            Command::ShowInbox | Command::ShowScheduled | Command::ShowProjects | Command::ShowLibrarian | Command::ShowHibernating | Command::ShowArchived => {
                self.unfold_threads(cx);
                match command {
                    Command::ShowScheduled => self.set_mode(SidebarMode::Scheduled, cx),
                    Command::ShowProjects => self.set_mode(SidebarMode::Projects, cx),
                    Command::ShowLibrarian => self.set_mode(SidebarMode::Librarian, cx),
                    Command::ShowHibernating => self.show_folded_section("hibernating", cx),
                    Command::ShowArchived => self.show_folded_section("archived", cx),
                    _ => self.set_mode(SidebarMode::Inbox, cx),
                }
            }
            Command::Settings => self.open_settings(None, cx),
            Command::ZoomIn => self.zoom_in(window, cx),
            Command::ZoomOut => self.zoom_out(window, cx),
            Command::ActualSize => self.set_zoom(zoom::DEFAULT_PERCENT, window, cx),
            Command::ToggleThreads => self.toggle_threads_panel(cx),
            Command::TogglePanels => self.toggle_panels(cx),
            Command::ToggleTerminal => crate::terminal::toggle(self, window, cx),
            Command::ResetPanelLayout => self.reset_panel_layout(cx),
            Command::CollapseThreads => self.toggle_threads_collapsed(cx),
            Command::ToggleFrameRate => self.toggle_frame_rate(window, cx),
            Command::About => self.show_about(window, cx),
            Command::RestartToUpdate => self.restart_to_update(cx),
            // Help's replay of the tour can be skipped (tour.rs).
            Command::TakeTheTour => self.tour_start(true, window, cx),
            Command::FinishSettingUp => self.show_setup(cx),
            Command::HibernateHour => self.hibernate_open("hour", window, cx),
            Command::HibernateEvening => self.hibernate_open("evening", window, cx),
            Command::HibernateTomorrow => self.hibernate_open("tomorrow", window, cx),
            Command::HibernateMonday => self.hibernate_open("monday", window, cx),
            Command::HibernateRestart => {
                if let Some(id) = self.selected.clone() {
                    self.snooze_until_restart(id, window, cx);
                }
            }
            // What's typed goes to disk on the way out (drafts.rs's
            // on_app_quit), once it's been asked (quit_prompt.rs).
            Command::Quit => crate::quit_prompt::quit(self.settings.asks_before_quitting(), cx.entity().downgrade(), cx),
            Command::Hide => cx.hide(),
            Command::HideOthers => cx.hide_other_apps(),
            Command::ShowAll => cx.unhide_other_apps(),
            Command::Minimize => window.minimize_window(),
            Command::Zoom => window.zoom_window(),
            Command::BringAllToFront => crate::window_menu::bring_all_to_front(cx),
            Command::Close => crate::window_menu::close_window(window, cx),
        }
        self.sync_menus(cx);
    }

    /// ⌘E: archives the open thread, or brings it back if it's archived
    /// (`toggleArchive`).
    pub(crate) fn toggle_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let archived = self.threads.get(&id).is_some_and(|t| t.is_archived());
        if archived {
            self.unarchive_row(&id, cx);
        } else {
            self.archive_row_sliding(id, window, cx);
        }
    }

    fn hibernate_open(&mut self, preset: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let until = threadbear_core::sidebar::snooze_date(preset, &threadbear_core::sidebar::local_now());
        self.snooze_row(id, until, window, cx);
    }

    /// A thread named from outside the list: the Inbox view, with its section
    /// unfolded if it's put away, as a chip in a reply does (`openThread`).
    pub(crate) fn reveal_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.threads.get(id) {
            let section = if t.is_archived() {
                Some("archived")
            } else if t.snoozed(crate::now_ms()) && !t.is_starred() {
                Some("hibernating")
            } else {
                None
            };
            if let Some(section) = section {
                self.folded.remove(section);
                self.sidebar_settings_changed(cx);
            }
        }
        self.mode = SidebarMode::Inbox;
        // Its row in view, wherever the list was scrolled to.
        self.reveal_row(id, cx);
        cx.notify();
    }

    /// Show Hibernating / Show Archived: the inbox view with that section open.
    fn show_folded_section(&mut self, section: &'static str, cx: &mut Context<Self>) {
        self.mode = SidebarMode::Inbox;
        self.folded.remove(section);
        self.sidebar_settings_changed(cx);
        self.note_navigation();
        cx.notify();
    }

    /// `app.sidebarMode = ...` and the pages' `showProjects()` etc.: the view
    /// on show, recorded for back and forward.
    /// The Scheduled, Projects and Librarian buttons open their page in the
    /// main panel too (`showSchedules`); picking a thread puts the thread back.
    pub(crate) fn set_mode(&mut self, mode: SidebarMode, cx: &mut Context<Self>) {
        let home = mode != SidebarMode::Inbox;
        // A page of the activity bar's takes a project's page's place
        // (`showProjects`); the Inbox leaves it where it is.
        let closes_project = home && self.projects.open.is_some();
        if self.mode != mode || self.shows_home != home || closes_project {
            if self.mode != mode {
                self.view_switched = Some(std::time::Instant::now());
            }
            self.mode = mode;
            self.shows_home = home;
            if closes_project {
                self.projects.open = None;
            }
            self.note_navigation();
        }
        cx.notify();
    }

    /// The page in the main panel in place of the thread, if one is.
    pub(crate) fn home_page(&self) -> Option<SidebarMode> {
        (self.shows_home && self.mode != SidebarMode::Inbox).then_some(self.mode)
    }

    // Back and forward (Navigation.swift).

    pub(crate) fn place(&self) -> NavPlace {
        NavPlace { mode: self.mode, thread: self.selected.clone(), home: self.home_page().is_some(), project: self.projects.open.clone() }
    }

    /// `noteNavigation`: one step per place the window has been. Not while
    /// Back or Forward is putting a place back: the steps on the way (the
    /// thread opened before its page goes back over it) aren't places, and
    /// recording one cut off the history ahead.
    pub(crate) fn note_navigation(&mut self) {
        if self.restoring_place {
            return;
        }
        let place = self.place();
        // The page in place of a thread, as the activity log names it.
        let page = (place.home && place.mode != SidebarMode::Inbox).then(|| place.mode.name());
        crate::activity::view(place.mode.name(), place.thread.as_deref(), self.subagent.as_ref().map(|(_, id)| id.as_str()), place.project.as_deref(), page);
        // ⌘K's Recent: the thread or project page now in the main panel.
        let recent = match (&place.project, &place.thread) {
            (Some(project), _) => Some(threadbear_core::open_anything::project_key(project)),
            (None, Some(thread)) if !place.home => Some(threadbear_core::open_anything::thread_key(thread)),
            _ => None,
        };
        if let Some(key) = recent.filter(|k| self.anything_recent.first() != Some(k)) {
            self.anything_recent = threadbear_core::open_anything::used(&key, &self.anything_recent);
            self.settings.saved.open_anything_recent = Some(self.anything_recent.clone());
        }
        self.history.record(place);
    }

    /// A place can be gone back to while what it showed is still there: its
    /// thread, unless a page was over it (the thread behind a page can go).
    pub(crate) fn can_show(&self, place: &NavPlace) -> bool {
        // A merged or deleted project's page is skipped rather than shown empty.
        if let Some(id) = &place.project {
            return self.project(id).is_some();
        }
        place.home || place.thread.as_ref().is_none_or(|id| self.threads.contains_key(id))
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.history.can_go_back()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.history.can_go_forward()
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let threads = self.threads.keys().cloned().collect::<std::collections::HashSet<_>>();
        let projects = self.projects.list.iter().map(|p| p.id.clone()).collect::<std::collections::HashSet<_>>();
        let can_show = |p: &NavPlace| match &p.project {
            Some(id) => projects.contains(id),
            None => p.home || p.thread.as_ref().is_none_or(|id| threads.contains(id)),
        };
        if let Some(place) = self.history.back(can_show) {
            self.show_place(place, window, cx);
        }
    }

    pub(crate) fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let threads = self.threads.keys().cloned().collect::<std::collections::HashSet<_>>();
        let projects = self.projects.list.iter().map(|p| p.id.clone()).collect::<std::collections::HashSet<_>>();
        let can_show = |p: &NavPlace| match &p.project {
            Some(id) => projects.contains(id),
            None => p.home || p.thread.as_ref().is_none_or(|id| threads.contains(id)),
        };
        if let Some(place) = self.history.forward(can_show) {
            self.show_place(place, window, cx);
        }
    }

    /// Puts the window back at `place`; the history already moved, so the step
    /// this records is where it is now and adds nothing.
    fn show_place(&mut self, place: NavPlace, window: &mut Window, cx: &mut Context<Self>) {
        self.restoring_place = true;
        self.mode = place.mode;
        // A page's thread behind it may have gone since.
        match place.thread.filter(|id| self.threads.contains_key(id)) {
            Some(id) => self.open(id, window, cx),
            None => {
                self.select_none(cx);
            }
        }
        // After the thread: opening one takes a page away.
        self.shows_home = place.home;
        self.projects.open = place.project.filter(|id| self.project(id).is_some());
        self.restoring_place = false;
        // What's showing is that entry now, even if it couldn't be shown just
        // as it was (the thread behind a page has gone): put in its place,
        // keeping Forward, rather than recorded as somewhere new.
        let now = self.place();
        self.history.settle_current(now);
        cx.notify();
    }

    /// Where Back or Forward would go, named for its tooltip (`destination`).
    pub(crate) fn destination(&self, back: bool) -> Option<String> {
        let place = self.history.peek(back, |p| self.can_show(p))?;
        if let Some(project) = place.project.as_deref().and_then(|id| self.project(id)) {
            return Some(project.title.clone());
        }
        // A page is named for itself; otherwise the thread, or the new-thread page.
        let page = place.home.then_some(place.mode).filter(|m| *m != SidebarMode::Inbox);
        Some(match (page, place.thread.and_then(|id| self.threads.get(&id).cloned())) {
            (Some(SidebarMode::Scheduled), _) => "Scheduled".to_string(),
            (Some(SidebarMode::Projects), _) => "Projects".to_string(),
            (Some(_), _) => "Librarian".to_string(),
            (None, Some(t)) if t.title.is_empty() => "Untitled".to_string(),
            (None, Some(t)) => t.title.clone(),
            (None, None) => "New thread".to_string(),
        })
    }

    /// Next Inbox Thread (⌥⌘↓, `nextInboxThread`): the thread below the open
    /// one in the Inbox, or the Inbox's first; past the last one, a note.
    pub(crate) fn next_inbox_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = crate::now_ms();
        let inbox: Vec<String> = self.shown_sections(cx, now).iter().find(|s| s.key == "inbox").map(|s| s.rows.iter().map(|t| t.id.clone()).collect()).unwrap_or_default();
        let next = match self.selected.as_ref().and_then(|s| inbox.iter().position(|id| id == s)) {
            Some(i) => inbox.get(i + 1).cloned(),
            None => inbox.first().cloned(),
        };
        let Some(next) = next else {
            self.archived_note = None;
            self.show_inbox_end(inbox.is_empty(), cx);
            return;
        };
        self.inbox_end_note = None;
        self.mode = SidebarMode::Inbox;
        // A folded Inbox has no row to show it by.
        if self.folded.remove("inbox") {
            self.sidebar_settings_changed(cx);
        }
        // Brought into view, as the arrow keys do (`revealSelectionRequests`).
        self.reveal_row(&next, cx);
        // Down the Inbox: in from below, as the down arrow (open_motion.rs).
        self.opening.next = Some(crate::open_motion::Opening::Step(1));
        self.open(next, window, cx);
    }

    pub(crate) fn show_inbox_end(&mut self, empty: bool, cx: &mut Context<Self>) {
        self.note_counter += 1;
        let id = self.note_counter;
        self.inbox_end_note = Some(InboxEndNote { id, empty });
        // Three seconds, then it goes (`InboxEndToast.shownFor`), unless the
        // pointer's on it.
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let keep = this.update(cx, |this, cx| {
                    if this.inbox_end_note.is_some_and(|n| n.id == id) {
                        if this.toast_hovered {
                            return true;
                        }
                        this.inbox_end_note = None;
                        cx.notify();
                    }
                    false
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        });
        self.note_task = Some(task);
        cx.notify();
    }

    pub(crate) fn dismiss_inbox_end(&mut self, cx: &mut Context<Self>) {
        self.inbox_end_note = None;
        cx.notify();
    }

    /// The archived note: six seconds, unless the pointer's on it
    /// (`ArchivedToast.shownFor`).
    pub(crate) fn show_archived_note(&mut self, thread: String, title: String, was_open: bool, cx: &mut Context<Self>) {
        self.note_counter += 1;
        let id = self.note_counter;
        self.archived_note = Some(ArchivedNote { id, thread, title, was_open });
        self.inbox_end_note = None;
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(6)).await;
                let keep = this.update(cx, |this, cx| {
                    if this.archived_note.as_ref().is_some_and(|n| n.id == id) {
                        if this.toast_hovered {
                            return true;
                        }
                        this.archived_note = None;
                        cx.notify();
                    }
                    false
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        });
        self.archived_task = Some(task);
        cx.notify();
    }

    /// The archived note's Undo: back in the inbox, and open again if it was
    /// the one open when it went (`undoArchive`).
    pub(crate) fn undo_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.archived_note.take() else { return };
        self.unarchive_row(&note.thread, cx);
        if note.was_open {
            self.mode = SidebarMode::Inbox;
            self.open(note.thread, window, cx);
        }
        cx.notify();
    }

    /// A click on the archived note: opens the thread where it now is
    /// (`openArchivedNote`).
    pub(crate) fn open_archived_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.archived_note.take() else { return };
        self.show_folded_section("archived", cx);
        self.open(note.thread, window, cx);
        cx.notify();
    }

    // Keys and menus.

    /// Applies the shortcuts in force: bindings added for what's new, and a
    /// `NoAction` on what's gone, since GPUI's bindings are additive and
    /// clearing the keymap would lose gpui-kit's own.
    pub(crate) fn apply_shortcuts(&mut self, cx: &mut App) {
        let macos = cfg!(target_os = "macos");
        let mut desired: HashMap<String, Command> = HashMap::new();
        for (chord, command) in standard_keys() {
            desired.insert(chord.to_owned(), command);
        }
        // An experiment's keys only while it's on (`ShortcutCommand::experiment`).
        let on = self.open_anything_on();
        self.bound_open_anything = on;
        for command in Command::ALL {
            let shortcut = command.shortcut().filter(|c| c.experiment().is_none() || on);
            if let Some(key) = shortcut.and_then(|c| self.shortcuts.key(c)) {
                desired.insert(key.chord(macos), *command);
            }
        }
        let mut added = Vec::new();
        for (chord, command) in &desired {
            if self.bound_keys.get(chord) != Some(command) {
                added.push(command.binding(chord));
            }
        }
        for chord in self.bound_keys.keys() {
            if !desired.contains_key(chord) {
                added.push(KeyBinding::new(leak(chord), NoAction, None));
            }
        }
        if !added.is_empty() {
            cx.bind_keys(added);
            // After them, so the terminal's own keys still win there
            // (terminal::own_keys says why).
            cx.bind_keys(crate::terminal::own_keys());
        }
        self.bound_keys = desired;
    }

    /// shortcuts.json changed (or the app just started): the keys in use, the
    /// keymap and the menus' keys follow it. One that won't parse leaves the
    /// keys in use alone: an agent may be halfway through editing it (`load`).
    fn reload_shortcuts(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        match text {
            None => self.shortcuts = ShortcutBindings::default(),
            Some(text) => {
                if let Ok(read) = ShortcutBindings::parse(&text) {
                    self.shortcuts = read;
                }
            }
        }
        self.apply_shortcuts(cx);
        self.menu_signature.clear();
        self.sync_menus(cx);
        cx.notify();
    }

    /// Watches shortcuts.json: a change to the file shows straight away, as
    /// the Swift app's DispatchSource watch does. A poll rather than a file
    /// watcher crate: the file is tiny and this needs no dependency.
    pub(crate) fn watch_shortcuts(&mut self, cx: &mut Context<Self>) {
        let path = self.settings.path().parent().map(|d| d.join("shortcuts.json")).unwrap_or_else(|| "shortcuts.json".into());
        self.reload_shortcuts(std::fs::read_to_string(&path).ok(), cx);
        self.shortcuts_path = Some(path.clone());
        self.shortcuts_watch = Some(cx.spawn(async move |this, cx| {
            let mut last = std::fs::read_to_string(&path).ok();
            loop {
                cx.background_executor().timer(Duration::from_millis(600)).await;
                let now = std::fs::read_to_string(&path).ok();
                if now != last {
                    last = now.clone();
                    if this.update(cx, |this, cx| this.reload_shortcuts(now, cx)).is_err() {
                        break;
                    }
                }
            }
        }));
    }

    /// The menu bar, rebuilt only when what it says changes (a title like
    /// Archive/Unarchive, a greyed-out item, a key).
    pub(crate) fn sync_menus(&mut self, cx: &mut Context<Self>) {
        let menus = self.menu_model(cx);
        let signature = serde_json::to_string(&menus_json(&menus, &self.shortcuts)).unwrap_or_default();
        if signature != self.menu_signature {
            self.menu_signature = signature;
            cx.set_menus(gpui_menus(&menus));
            #[cfg(target_os = "macos")]
            unsafe {
                extern "C" { fn tb_menu_icons(); }
                tb_menu_icons();
            }
        }
    }

    /// The window's title (Window menu, Mission Control), as `windowTitle`.
    pub(crate) fn sync_window_title(&mut self, window: &mut Window) {
        let title = self.main_panel_title();
        if title != self.last_window_title {
            window.set_window_title(&title);
            self.last_window_title = title;
        }
    }

    fn menu_model(&self, cx: &App) -> Vec<TopMenu> {
        let t = self.open_thread();
        let has_thread = t.is_some();
        let zoom_now = style::zoom_percent();
        let panels_shown = !self.open_panes(cx).is_empty();
        let threads_showing = self.threads_showing(cx);
        let terminal_shown = self.is_panel_showing(TERMINAL, cx);
        let archived = t.is_some_and(|t| t.is_archived());
        let busy = t.is_some_and(|t| t.busy());
        use MenuEntry::*;
        let item = |title: &str, command: Command| MenuEntry::item(title, command);
        // Restart to Update is there only while a newer build is staged
        // (app_update.rs), under About, as the Swift app has it.
        let restart = self.updater.staged.as_ref().map(|s| item(&format!("Restart to Update to {}", s.short), Command::RestartToUpdate));
        vec![
            TopMenu::new(
                "Threadbear",
                [item("About Threadbear", Command::About)]
                    .into_iter()
                    .chain(restart)
                    .chain([
                        Separator,
                        item("Settings…", Command::Settings),
                        Separator,
                    ])
                    // The system's Services, for the words selected in a
                    // text box (a Mac's own; the Swift app has it).
                    .chain(cfg!(target_os = "macos").then_some([Services, Separator]).into_iter().flatten())
                    .chain([
                        item("Hide Threadbear", Command::Hide),
                        item("Hide Others", Command::HideOthers),
                        item("Show All", Command::ShowAll),
                        Separator,
                        item("Quit Threadbear", Command::Quit),
                    ])
                    .collect(),
            ),
            TopMenu::new("File", vec![item("New Thread", Command::NewThread), Separator, item("Close", Command::Close)]),
            // The text commands, as AppKit's standard Edit menu has them;
            // AppKit adds AutoFill, Start Dictation and Emoji & Symbols
            // under them on a Mac (window_menu.rs).
            TopMenu::new(
                "Edit",
                {
                    use crate::window_menu::EditItem as E;
                    let edit = |e: E| MenuEntry::Edit(e);
                    vec![edit(E::Undo), edit(E::Redo), Separator, edit(E::Cut), edit(E::Copy), edit(E::Paste), edit(E::PasteAndMatchStyle), edit(E::Delete), edit(E::SelectAll)]
                },
            ),
            TopMenu::new(
                "View",
                vec![
                    item("Zoom In", Command::ZoomIn).enabled(zoom::can_zoom_in(zoom_now)),
                    item("Zoom Out", Command::ZoomOut).enabled(zoom::can_zoom_out(zoom_now)),
                    item("Actual Size", Command::ActualSize).enabled(zoom_now != zoom::DEFAULT_PERCENT),
                    Separator,
                    item(if threads_showing { "Hide Threads" } else { "Show Threads" }, Command::ToggleThreads),
                    // Folds the panel away and back as it was; the activity bar stays.
                    item(if threads_showing { "Collapse Threads" } else { "Expand Threads" }, Command::CollapseThreads),
                    item(if panels_shown { "Hide Panels" } else { "Show Panels" }, Command::TogglePanels),
                    item(if terminal_shown { "Hide Terminal" } else { "Show Terminal" }, Command::ToggleTerminal),
                    item("Reset Panel Layout", Command::ResetPanelLayout),
                ]
                .into_iter()
                .chain(self.frame_rate_items())
                .collect(),
            ),
            TopMenu::new(
                "Go",
                vec![
                    item("Back", Command::Back).enabled(self.can_go_back()),
                    item("Forward", Command::Forward).enabled(self.can_go_forward()),
                    Separator,
                    item("Next Inbox Thread", Command::NextInboxThread),
                    // A project's page tabs (the projectDecisions experiment),
                    // there always and greyed out away from one, as the Swift
                    // app's Go menu has them.
                    Separator,
                    item("Show Previous Tab", Command::PreviousTab).enabled(self.can_step_project_tab(-1, cx)),
                    item("Show Next Tab", Command::NextTab).enabled(self.can_step_project_tab(1, cx)),
                ]
                .into_iter()
                .chain(self.open_anything_on().then(|| [Separator, item("Open Anything…", Command::OpenAnything)]).into_iter().flatten())
                .collect(),
            ),
            TopMenu::new(
                "Thread",
                vec![
                    item(if archived { "Unarchive" } else { "Archive" }, Command::Archive).enabled(has_thread),
                    Submenu { title: "Hibernate".into(), items: self.hibernate_menu_items() },
                    item("Stop", Command::Stop).enabled(busy),
                    Separator,
                    item("Copy Transcript as Markdown", Command::CopyTranscript).enabled(has_thread),
                    Separator,
                    item("Show Inbox", Command::ShowInbox),
                    item("Show Scheduled", Command::ShowScheduled).enabled(self.schedules_supported),
                    item("Show Projects", Command::ShowProjects).enabled(self.projects_supported),
                    item("Show Librarian", Command::ShowLibrarian).enabled(self.projects_supported),
                    Separator,
                    item("Show Hibernating", Command::ShowHibernating),
                    item("Show Archived", Command::ShowArchived),
                ],
            ),
            // AppKit adds Fill, Center, Move & Resize and the list of open
            // windows (the main one and any panel in a window of its own).
            TopMenu::new("Window", vec![item("Minimize", Command::Minimize), item("Zoom", Command::Zoom), Separator, item("Bring All to Front", Command::BringAllToFront)]),
            TopMenu::new("Help", vec![item("Take the Tour", Command::TakeTheTour), item("Finish Setting Up", Command::FinishSettingUp)]),
        ]
    }

    /// Show Frame Rate, in a build that can measure it (`frame-stats`).
    fn frame_rate_items(&self) -> Vec<MenuEntry> {
        #[cfg(feature = "frame-stats")]
        {
            let title = if self.frame_rate.is_some() { "Hide Frame Rate" } else { "Show Frame Rate" };
            vec![MenuEntry::Separator, MenuEntry::item(title, Command::ToggleFrameRate)]
        }
        #[cfg(not(feature = "frame-stats"))]
        Vec::new()
    }

    fn toggle_frame_rate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature = "frame-stats")]
        {
            self.frame_rate = match self.frame_rate.take() {
                Some(_) => None,
                None => Some(cx.new(|cx| crate::frame_stats::FrameRate::new(window, cx))),
            };
            cx.notify();
        }
        let _ = (window, cx);
    }

    fn hibernate_menu_items(&self) -> Vec<MenuEntry> {
        let presets = threadbear_core::sidebar::snooze_presets(&threadbear_core::sidebar::local_now());
        let mut items: Vec<MenuEntry> = presets
            .iter()
            .filter_map(|p| {
                let command = match p.id {
                    "hour" => Command::HibernateHour,
                    "evening" => Command::HibernateEvening,
                    "tomorrow" => Command::HibernateTomorrow,
                    "monday" => Command::HibernateMonday,
                    _ => return None,
                };
                Some(MenuEntry::item(p.title, command))
            })
            .collect();
        // Only while a runner restart is waiting, as the Swift menu has it.
        if self.client.restart().is_pending() {
            items.push(MenuEntry::item(threadbear_core::sidebar::SNOOZE_UNTIL_RESTART, Command::HibernateRestart));
        }
        items.push(MenuEntry::Separator);
        items.push(MenuEntry::item("Pick a time…", Command::Hibernate));
        items.push(MenuEntry::item(threadbear_core::sidebar::WAKE_MENU_TITLE, Command::WakeWhen));
        items
    }

    /// Every menu as data, for the specs and `app-menu` (Automation's `menus`).
    #[cfg(feature = "automation")]
    pub(crate) fn menus_for_dump(&self, cx: &App) -> Value {
        let mut out = menus_json(&self.menu_model(cx), &self.shortcuts);
        // And what AppKit put in each menu by itself (Emoji & Symbols, Enter
        // Full Screen, the open windows), as the Swift app's `menus` reads
        // the real menu bar, which has them.
        for top in out.as_array_mut().into_iter().flatten() {
            let title = top["title"].as_str().unwrap_or_default().to_owned();
            let ours: Vec<String> = top["items"].as_array().into_iter().flatten().filter_map(|i| i["title"].as_str().map(str::to_owned)).collect();
            let extra = crate::window_menu::system_items(&title, &ours);
            if let Some(items) = top["items"].as_array_mut() {
                items.extend(extra.into_iter().map(|t| serde_json::json!({ "title": t, "enabled": true, "system": true })));
            }
        }
        out
    }

    #[cfg(feature = "automation")]
    pub(crate) fn run_menu_item(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let menus = self.menu_model(cx);
        enum Found {
            Command(Command),
            Edit(crate::window_menu::EditItem),
        }
        fn find(entries: &[MenuEntry], title: &str) -> Option<Found> {
            entries.iter().find_map(|e| match e {
                MenuEntry::Item { title: t, command, .. } if t == title => Some(Found::Command(*command)),
                MenuEntry::Edit(item) if item.title() == title => Some(Found::Edit(*item)),
                MenuEntry::Submenu { items, .. } => find(items, title),
                _ => None,
            })
        }
        match menus.iter().find_map(|m| find(&m.items, title)).ok_or_else(|| format!("no menu item {title:?}"))? {
            Found::Command(command) => self.command_chosen(command, window, cx),
            // To wherever the keyboard is, as the menu bar sends it.
            Found::Edit(item) => {
                let action = item.action();
                cx.defer_in(window, move |_, window, cx| window.dispatch_action(action, cx));
            }
        }
        Ok(())
    }

    /// "Thread › Archive": the menu and the item a command is, for the
    /// activity log, or none for a command no menu has.
    pub(crate) fn menu_path(&self, command: Command, cx: &App) -> Option<String> {
        fn find(parent: &str, entries: &[MenuEntry], command: Command) -> Option<String> {
            entries.iter().find_map(|e| match e {
                MenuEntry::Item { title, command: c, .. } if *c == command => Some(format!("{parent} › {title}")),
                MenuEntry::Submenu { title, items } => find(title, items, command),
                _ => None,
            })
        }
        self.menu_model(cx).iter().find_map(|m| find(&m.title, &m.items, command))
    }

    // The shell's drawing.

    pub(crate) fn render_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        self.sync_window_title(window);
        // The layer the sidebar's arcs turn in (spinner.rs), which looks at
        // the dock to see when the Threads panel isn't in front.
        let (dock, dock_view) = (self.dock.clone(), self.dock_view.clone());
        let arc_layer = self.arc_layer.get_or_insert_with(|| cx.new(|_| crate::spinner::ArcLayer::new(dock, dock_view, THREADS))).clone();
        // With nothing focused GPUI's key dispatch doesn't reach this element's
        // key context or actions, so the shell keeps the keyboard when nothing
        // else has it (no thread open, a closed dialog).
        // Leaves AppKit's keyboard where it is: a drawing's, not a request.
        if window.focused(cx).is_none() {
            window.focus(&self.root_focus, cx);
        }
        // On a project's page, the panels it borrows from one of its threads.
        // What the tour and the intros watch for (tour.rs).
        self.tour_watch(window, cx);
        let has_rail = self.pane_thread().is_some();
        let rail = has_rail.then(|| self.render_right_rail(cx));
        let root = div()
            .id("threadbear-shell")
            .track_focus(&self.root_focus)
            .flex()
            .flex_row()
            .key_context("Threadbear")
            .size_full()
            .items_start()
            // Two fingers across the window go back and forward, with a card
            // in the middle saying where (swipe_nav.rs); a press on a sidebar
            // row is watched to see if it's a swipe or the thread dragged out
            // (row_swipe.rs). First, as the capture phase goes in the order
            // things are painted: they hear the events before anything else
            // does, as the Swift app's monitors do, and a scroll view's own
            // (the kit's bounce, which keeps a short scroll's momentum from
            // the list) can't take the start of a swipe from them.
            .child(self.page_swipe_watch(cx))
            .child(self.row_press_watch(cx))
            .child(self.render_activity_bar(window, cx))
            .child(div().relative().flex_1().min_w_0().h_full().child(self.dock_view.clone()).child(crate::tour::dock_origin_mark()))
            .when_some(rail, |s, r| s.child(r))
            // An archived or hibernated thread's page on its way to its
            // heading, over the sidebar and the panel alike (archive_flight.rs).
            // The sidebar's turning arcs, drawn here so a turn doesn't
            // redraw the sidebar (spinner.rs).
            .child(div().absolute().top_0().left_0().size_full().child(arc_layer))
            .child(self.flight.overlay(cx))
            .children(self.render_swipe_card(cx).map(|card| deferred(card).with_priority(1)))
            // The tour and the pages' intros, over everything (tour.rs).
            .children(self.render_tour(window, cx))
            // A click or a key in the window means he's at it, for a window
            // the system never tells it's in front of (an X server with no
            // window manager): reading needs one (`looking_changed`).
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                if !crate::thread::is_looking() && !crate::thread::input_is_scripted() {
                    this.looking_changed(crate::thread::window_key(window), true, cx);
                }
            }))
            .capture_key_down(cx.listener(|this, _: &KeyDownEvent, window, cx| {
                if !crate::thread::is_looking() && !crate::thread::input_is_scripted() {
                    this.looking_changed(crate::thread::window_key(window), true, cx);
                }
            }))
            // The mouse's side buttons (`MouseNavigation`).
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), cx.listener(|this, _: &MouseDownEvent, window, cx| this.go_back(window, cx)))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), cx.listener(|this, _: &MouseDownEvent, window, cx| this.go_forward(window, cx)))
            // Holding ⌘ shows the buttons' keys; a key, a click or a scroll
            // with it held puts them away (key_hints.rs).
            .on_modifiers_changed(cx.listener(|this, ev: &ModifiersChangedEvent, window, cx| this.hints_modifiers_changed(&ev.modifiers, window, cx)))
            .capture_any_mouse_down(cx.listener(|this, ev: &MouseDownEvent, _, cx| this.hints_used(&ev.modifiers, cx)))
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _, cx| this.hints_used(&ev.modifiers, cx)));
        register_commands(root, cx)
    }

    /// What a panel shows (`RootView.content`'s `view`).
    pub(crate) fn panel_view(&mut self, panel: &str, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match panel {
            // A drag's ghost of it is drawn in place, not from the cached view,
            // which is already drawn where the panel sits.
            THREADS if ghost => self.render_threads_panel(window, cx),
            THREADS => {
                let app = cx.entity();
                let view = self.sidebar_view.get_or_insert_with(|| cx.new(|cx| crate::sidebar::SidebarView::new(&app, cx))).clone();
                AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
            }
            MAIN if ghost => self.render_main_panel(window, cx).into_any_element(),
            MAIN => {
                let app = cx.entity();
                let view = self.main_view.get_or_insert_with(|| cx.new(|cx| MainPanelView::new(&app, cx))).clone();
                AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
            }
            RUNNERS => self.activity_panel_view(RUNNERS, ghost, window, cx),
            UPDATES => self.activity_panel_view(UPDATES, ghost, window, cx),
            CONNECTORS => self.activity_panel_view(CONNECTORS, ghost, window, cx),
            SETUP => self.activity_panel_view(SETUP, ghost, window, cx),
            USAGE => self.usage_panel(ghost, window, cx),
            // The Terminal (terminal.rs): the open thread's shell. A drag's
            // ghost is the empty panel: the shell's view is drawn in one place.
            TERMINAL if ghost => div().size_full().into_any_element(),
            TERMINAL => crate::terminal::panel_view(cx),
            // A thread in a panel of its own (thread_panels.rs).
            _ if crate::thread_panels::pinned_thread(panel).is_some() => self.thread_panel_view(panel, ghost, window, cx),
            // A panel pinned to its thread (pinned_panels.rs).
            _ if crate::pinned_panels::pinned(panel).is_some() => self.pinned_panel_view(panel, ghost, window, cx),
            // Highlights, Files, Patches and a patch's own panels are sections of
            // the one pane beside the thread (highlights.rs), each its own panel
            // in the dock, as in the Swift app.
            _ if !ghost && self.is_side_section(panel) => self.render_side_section(panel, window, cx),
            _ => self.render_pane_placeholder(panel, cx).into_any_element(),
        }
    }

    /// What the Threads panel shows: the thread list, or the page of the
    /// rail's other modes (drawn by `SidebarView`).
    pub(crate) fn render_threads_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = match self.mode {
            SidebarMode::Inbox => self.render_sidebar(cx).into_any_element(),
            SidebarMode::Scheduled => self.render_scheduled_list(cx),
            SidebarMode::Librarian => self.render_librarian_list(cx),
            SidebarMode::Projects => self.render_projects_list(cx),
        };
        // A view the activity bar's just switched to fades up into place
        // (SidebarView's transition; Mike picked it, 13 Nov). Under a wrapper
        // keyed by the switch, so each one starts over, and kept once it's
        // done, so the view's own state isn't remade.
        let reduce = cx.reduce_motion();
        let Some(at) = self.view_switched else { return view };
        let length = threadbear_core::juice::VIEW_SETTLE;
        div()
            .relative()
            .size_full()
            .child(view)
            .with_animation(SharedString::from(format!("view-settle-{at:?}")), Animation::new(length), move |el, t| {
                let (fade, below) = threadbear_core::juice::view_settle(t * length.as_secs_f32(), reduce);
                el.opacity(fade).top(px(below))
            })
            .into_any_element()
    }

    /// The main panel: banners over the thread, the notes floating over it.
    pub(crate) fn render_main_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.home_page();
        let banners = self.render_banners(window, cx);
        // The new-thread page comes up afresh each time it's shown
        // (open_motion.rs): anything else in its place resets its clock.
        if self.pane.is_some() || page.is_some() || self.open_project().is_some() || self.open_elsewhere(cx).is_some() {
            self.opening.page_shown = None;
        }
        let main: AnyElement = match (&self.pane, page) {
            // A project's page takes the thread's place until a thread is
            // picked again (project_page.rs).
            _ if self.open_project().is_some() => self.render_project_page(window, cx),
            (Some(pane), None) => match self.subagent_pane.clone() {
                // One of its sub-agents, under a bar back to the thread (`SubagentHeader`).
                Some(sub) => v_flex().size_full().child(self.render_subagent_header(cx)).child(div().flex_1().min_h_0().child(sub)).into_any_element(),
                None => div().size_full().child(pane.clone()).into_any_element(),
            },
            (_, Some(SidebarMode::Scheduled)) => self.render_scheduled_home(cx),
            (_, Some(SidebarMode::Librarian)) => self.render_librarian_home(cx),
            (_, Some(_)) => self.render_projects_home(window, cx),
            // Its thread is open in a panel of its own (thread_panels.rs).
            (None, None) if self.open_elsewhere(cx).is_some() => {
                let id = self.open_elsewhere(cx).unwrap_or_default();
                self.render_open_elsewhere(id, cx)
            }
            // No thread open is the new-thread page, as in the Swift app.
            (None, None) => self.render_new_page(window, cx),
        };
        let toasts = self.render_toasts(window, cx);
        div().relative().size_full().child(v_flex().size_full().child(banners).child(div().flex_1().min_h_0().child(main))).child(toasts)
    }

    fn render_pane_placeholder(&mut self, panel: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let name = self.panel_title(panel);
        let text = if self.open_thread().is_some() { format!("{name} isn't in this app yet.") } else { format!("Pick a thread to see its {name}.") };
        div().size_full().flex().items_start().justify_center().pt(px(40.)).text_size(px(style::size::LABEL)).text_color(muted).child(text)
    }

    /// Whether a panel is one of the side pane's sections: Highlights, Files,
    /// Patches or one of the thread's patch panels (highlights.rs).
    fn is_side_section(&self, panel: &str) -> bool {
        matches!(panel, HIGHLIGHTS | FILES | PATCHES | BROWSER) || self.pane_thread().is_some_and(|t| t.panels.iter().flatten().any(|p| p["id"].as_str() == Some(panel)))
    }

    /// One section of the side pane, drawn by a view of its own so two side
    /// by side each show theirs (side_sections.rs).
    fn render_side_section(&mut self, section: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let side = self.side.clone();
        let view = self.side_views.entry(section.to_owned()).or_insert_with(|| cx.new(|cx| crate::side_sections::SectionView::new(side, section.to_owned(), cx))).clone();
        if let Some(thread) = self.pane_thread() {
            // A page panel draws its page itself, every frame, so the page
            // follows its slot (web_panels.rs); a preview over it is the pane's.
            let previewing = self.side.read(cx).previews.get(&thread.id).is_some_and(|pv| pv.from == section);
            if !previewing {
                if let Some(page) = crate::web_panels::section(self, &thread, section, false, window, cx) {
                    return page;
                }
            }
            // A video or a PDF in the system's view is placed where the pane
            // draws it every frame (media_view.rs), which a cached pane
            // wouldn't do, so it's drawn afresh while one's showing. It's
            // then only a bar and the view.
            if previewing && crate::files::shows_media(self.side.read(cx), &thread.id) {
                return AnyView::from(view).into_any_element();
            }
        }
        // Cached, like the Threads panel, so scrolling the transcript doesn't
        // redraw it every frame: it draws again when the app changes
        // (SidePane::app_changed) or the pane's own state does.
        AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
    }
}

/// `KeyBinding::new` wants `&'static str` keystrokes; a chord only changes
/// when shortcuts.json does, so the few bytes each change leaks don't matter.
fn leak(chord: &str) -> &'static str {
    Box::leak(chord.to_string().into_boxed_str())
}

/// The app's theme fonts for this zoom: gpui-kit's rem sizes and everything
/// built on them (menus, inputs, tooltips) follow.
pub(crate) fn apply_zoom_to_theme(percent: u32, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_size = gpui_kit::px(zoom::scaled(16., percent));
    theme.mono_font_size = gpui_kit::px(zoom::scaled(13., percent));
}

/// One top-level menu: a model so the menu bar, `menus` and `menu-item` all
/// read the same thing.
pub(crate) struct TopMenu {
    title: String,
    items: Vec<MenuEntry>,
}

impl TopMenu {
    fn new(title: &str, items: Vec<MenuEntry>) -> Self {
        Self { title: title.to_string(), items }
    }
}

pub(crate) enum MenuEntry {
    Item { title: String, command: Command, enabled: bool },
    Submenu { title: String, items: Vec<MenuEntry> },
    /// An Edit menu item: the text box's own action (window_menu.rs).
    Edit(crate::window_menu::EditItem),
    /// The system's Services submenu, filled by macOS.
    Services,
    Separator,
}

impl MenuEntry {
    fn item(title: &str, command: Command) -> Self {
        MenuEntry::Item { title: title.to_string(), command, enabled: true }
    }

    fn enabled(mut self, on: bool) -> Self {
        if let MenuEntry::Item { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }
}

fn menus_json(menus: &[TopMenu], shortcuts: &ShortcutBindings) -> Value {
    fn entries(items: &[MenuEntry], shortcuts: &ShortcutBindings) -> Vec<Value> {
        items
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item { title, command, enabled } => {
                    let key = command.shortcut().and_then(|c| shortcuts.key(c)).map(|k| k.display()).or_else(|| standard_key_display(*command).map(str::to_owned));
                    let mut out = serde_json::json!({ "title": title, "enabled": enabled });
                    if let Some(key) = key {
                        out["key"] = Value::String(key);
                    }
                    Some(out)
                }
                MenuEntry::Submenu { title, items } => Some(serde_json::json!({ "title": title, "items": entries(items, shortcuts) })),
                MenuEntry::Edit(item) => {
                    let mut out = serde_json::json!({ "title": item.title(), "enabled": true });
                    if let Some(key) = item.key() {
                        out["key"] = Value::String(key.to_owned());
                    }
                    Some(out)
                }
                MenuEntry::Services => Some(serde_json::json!({ "title": "Services", "enabled": true, "items": [] })),
                MenuEntry::Separator => None,
            })
            .collect()
    }
    Value::Array(menus.iter().map(|m| serde_json::json!({ "title": m.title, "items": entries(&m.items, shortcuts) })).collect())
}

fn gpui_menus(menus: &[TopMenu]) -> Vec<Menu> {
    fn entries(items: &[MenuEntry]) -> Vec<MenuItem> {
        items
            .iter()
            .map(|e| match e {
                MenuEntry::Item { title, command, enabled } => MenuItem::Action { name: title.clone().into(), action: command.action(), os_action: None, checked: false, disabled: !enabled },
                MenuEntry::Submenu { title, items } => MenuItem::submenu(Menu::new(title.clone()).items(entries(items))),
                MenuEntry::Edit(item) => MenuItem::Action { name: item.title().into(), action: item.action(), os_action: item.os_action(), checked: false, disabled: false },
                MenuEntry::Services => MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuEntry::Separator => MenuItem::Separator,
            })
            .collect()
    }
    menus.iter().map(|m| Menu::new(m.title.clone()).items(entries(&m.items))).collect()
}

/// The main panel as a view of its own, as the Threads panel is
/// (sidebar.rs's `SidebarView`), so the dock reuses its last frame when only
/// something beside it changed. A turning spinner in the sidebar asks for a
/// frame many times a second, and each one laid the open thread out again,
/// its whole transcript, with the rest of the window. Everything it draws is
/// the app's state, so it draws again whenever the app notifies, or something
/// in it does (the thread, its box).
pub(crate) struct MainPanelView {
    app: WeakEntity<Threadbear>,
    _app_changed: Subscription,
}

impl MainPanelView {
    fn new(app: &Entity<Threadbear>, cx: &mut Context<Self>) -> Self {
        let app_changed = cx.observe(app, |_, _, cx| cx.notify());
        Self { app: app.downgrade(), _app_changed: app_changed }
    }
}

impl Render for MainPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.render_main_panel(window, cx).into_any_element()),
            None => div().into_any_element(),
        }
    }
}
