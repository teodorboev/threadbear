// The app's state: the connection, the thread list, what's open, and the
// window's shell around it (shell.rs, rails.rs, header.rs). The sidebar
// (sidebar.rs) and the thread pane (thread.rs) draw from it.
use crate::composer::Composer;
use crate::settings::Settings;
use crate::shell::{self, ArchivedNote, InboxEndNote, NavPlace, SidebarMode};
use crate::highlights::SidePane;
use crate::thread::ThreadPane;
use crate::{call, now_ms, ChannelSink, Seen, RT};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Theme;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use threadbear_client::config::RunnerConfig;
use threadbear_client::runner::{Client, Status};
use threadbear_core::inbox::{self, iso_time, ThreadSummary};
use threadbear_core::logins::RunnerLogins;
use threadbear_core::navigation::NavigationHistory;
use threadbear_core::shortcuts::ShortcutBindings;
use threadbear_dock::{DockLayout, DockModel, DockView};
use tokio::sync::mpsc;

pub struct Threadbear {
    pub(crate) client: Arc<Client>,
    pub(crate) status: Status,
    pub(crate) threads: HashMap<String, ThreadSummary>,
    pub(crate) list_error: Option<String>,
    pub(crate) selected: Option<String>,
    pub(crate) pane: Option<Entity<ThreadPane>>,
    /// The open pane's requests to the app (a thread: link was clicked).
    pub(crate) pane_events: Option<Subscription>,
    /// The threads opened lately, kept loaded so switching back is at once
    /// (pane_cache.rs).
    pub(crate) kept: crate::pane_cache::PaneCache,
    pub(crate) search: Entity<InputState>,
    /// The sections folded away under their headings, by key.
    pub(crate) folded: HashSet<&'static str>,
    /// The runner's accounts, pools and defaults, from its `hello`, for the
    /// composer's chips.
    pub(crate) logins: RunnerLogins,
    /// Each thread's message box, kept so what's typed (with its undo and
    /// pictures) stays with the thread while others are open.
    pub(crate) composers: HashMap<String, Entity<Composer>>,
    /// What every box has, kept across a quit (drafts.rs).
    pub(crate) drafts: crate::drafts::Drafts,
    pub(crate) sidebar: crate::sidebar::SidebarState,

    // The window shell (shell.rs, rails.rs, header.rs).
    /// Holds the keyboard when nothing in the window has it (no thread open,
    /// say), so the app's keys and menus still reach the shell's actions.
    pub(crate) root_focus: FocusHandle,
    pub(crate) settings: Settings,
    pub(crate) settings_dirty: bool,
    pub(crate) settings_last_save_ms: i64,
    pub(crate) settings_save_task: Option<Task<()>>,
    pub(crate) dock: Entity<DockModel>,
    pub(crate) dock_view: Entity<DockView>,
    /// View > Show Frame Rate's readout, while it's showing.
    #[cfg(feature = "frame-stats")]
    pub(crate) frame_rate: Option<Entity<crate::frame_stats::FrameRate>>,
    /// The pane sections Hide Panels closed, for Show Panels (`hiddenPanes`).
    pub(crate) hidden_panes: Vec<String>,
    pub(crate) mode: SidebarMode,
    pub(crate) history: NavigationHistory<NavPlace>,
    /// shortcuts.json's keys, and the chords bound to them so far.
    pub(crate) shortcuts: ShortcutBindings,
    pub(crate) bound_keys: HashMap<String, shell::Command>,
    pub(crate) shortcuts_watch: Option<Task<()>>,
    pub(crate) shortcuts_path: Option<PathBuf>,
    /// What the menu bar says, so it's rebuilt only when that changes.
    pub(crate) menu_signature: String,
    /// Something changed since the menu bar was last worked out. It's
    /// rebuilt only then, not every frame: scrolling redraws the window as
    /// fast as the screen refreshes, and this was a noticeable part of each
    /// frame (set by an observer of the app itself, in `new`).
    pub(crate) menus_stale: bool,
    /// The login each thread was on when it went into error
    /// (`failedOnAccount`), so the limit line blames that login only while
    /// the thread's still on it, not the one it's just been switched to.
    pub(crate) failed_on_account: HashMap<String, String>,
    /// The login each thread's limit box has picked in its Continue now on
    /// menu, when it isn't the first.
    pub(crate) limit_pick: HashMap<String, String>,
    /// The Threads button's badge, kept until the app next changes (the same
    /// observer clears it), as working it out sorts every thread.
    pub(crate) inbox_count_cache: std::cell::Cell<Option<usize>>,
    pub(crate) last_window_title: String,
    pub(crate) archived_note: Option<ArchivedNote>,
    pub(crate) inbox_end_note: Option<InboxEndNote>,
    /// What came across from the Swift app, while its note's up (swift_import.rs).
    pub(crate) import_note: Option<String>,
    /// Following main: what's running, staged, building (app_update.rs).
    pub(crate) updater: crate::app_update::AppUpdater,
    pub(crate) note_counter: u64,
    pub(crate) note_task: Option<Task<()>>,
    pub(crate) archived_task: Option<Task<()>>,
    /// The pointer's on a note, which keeps it up.
    pub(crate) toast_hovered: bool,
    /// The error bar's text (`errorMessage`).
    pub(crate) error_message: Option<String>,
    /// The error bar's text when it's about not being connected, so it goes
    /// once the app is again (`connectionErrorMessage`).
    pub(crate) connection_error: Option<String>,
    /// What clicks from a menu are doing, by their key, newest last, and
    /// what's just been done, for the banner (`busyNotes`, `doneNote`).
    pub(crate) busy_notes: Vec<(String, String)>,
    pub(crate) done_note: Option<String>,
    pub(crate) done_task: Option<Task<()>>,
    /// CLI updates that failed and were dismissed from the banner, by
    /// `cli_failure_key` (`dismissedCliFailures`).
    pub(crate) dismissed_cli_failures: HashSet<String>,
    /// Asks `runner.status` which threads a pending restart waits for, as
    /// they finish, once for a burst of updates (`refreshRunnerStatus`).
    pub(crate) restart_status_task: Option<Task<()>>,
    /// Redraws the banner when a restart has gone on so long that the runner
    /// looks down (`restartExpiry`).
    pub(crate) restart_expiry: Option<Task<()>>,
    pub(crate) creating_investigation: bool,
    /// The title being renamed in the header, and its field.
    pub(crate) renaming: Option<String>,
    pub(crate) rename_field: Option<(Entity<InputState>, Subscription)>,
    pub(crate) share_popover: Option<String>,
    pub(crate) share_busy: bool,
    /// Threads whose login, pool ("moving/<id>") or folder
    /// ("movingFolder/<id>") change hasn't landed: those can't be guessed,
    /// so the composer's chips that show them spin until the runner answers
    /// (`BusyKey.moving`, `BusyKey.movingFolder`).
    pub(crate) chip_busy: std::collections::HashSet<String>,
    pub(crate) continuing_refusal: Option<String>,
    /// The runner's website, from `runner.status` (`websiteURL`).
    pub(crate) website_url: Option<String>,
    pub(crate) schedules_supported: bool,
    /// The Scheduled view: the list, its folded headings, what's on the way
    /// (scheduled.rs).
    pub(crate) scheduled: crate::scheduled::ScheduledState,
    /// The schedule editor, while it's up (schedule_editor.rs).
    pub(crate) schedule_editor: Option<Entity<crate::schedule_editor::ScheduleEditor>>,
    /// The activity bar's page for the sidebar's view is in the main panel
    /// (`showsScheduledHome` and friends), not the selected thread. Picking a
    /// thread puts the thread back.
    pub(crate) shows_home: bool,
    /// Back or Forward is putting a place back (`navigationPending`): the
    /// steps on the way aren't recorded.
    pub(crate) restoring_place: bool,
    /// The Librarian view (librarian.rs).
    pub(crate) librarian: crate::librarian::LibrarianState,
    /// What the Librarian page's picture says when it's poked.
    pub(crate) librarian_pokes: crate::home_page::Pokes,
    /// The Usage panel's numbers and the honey pot (usage.rs).
    pub(crate) usage: crate::usage::UsageState,
    /// The runner's settings (`settings.get`, then `settings.updated`), as
    /// they came: the Librarian page's Runs on reads them, Settings will.
    pub(crate) runner_settings: Option<Value>,
    /// The providers whose logins can run small jobs (titles, the
    /// librarian), from hello's `providers` (`ProviderInfo.smallJobs`).
    pub(crate) small_jobs: Vec<String>,
    /// The Settings window while it's open (settings_window.rs).
    pub(crate) settings_window: Option<(AnyWindowHandle, Entity<crate::settings_window::SettingsWindow>)>,
    /// The main window, for opening a thread from outside it (a
    /// notification's click).
    pub(crate) main_window: AnyWindowHandle,
    /// A thread a notification's click asked for before the list had
    /// loaded: opened once it has (`linkedThread`).
    pub(crate) pending_open: Option<String>,
    /// The thread that was open when the app restarted itself onto an
    /// update, opened once the list has loaded, in the list that was showing
    /// and without bringing the window in front (app_update.rs).
    pub(crate) pending_restore: Option<String>,
    /// A `threadbear://` link's thread waiting for the lists it could be in,
    /// and since when (deep_links.rs).
    pub(crate) waiting_link: Option<(String, std::time::Instant)>,
    /// What the Dock badge says now (notify.rs), so it's only set when that changes.
    pub(crate) dock_badge: Option<String>,
    /// The sub-agent whose transcript is in the main panel, by its thread and
    /// id, and its pane (`subagentSelection`).
    pub(crate) subagent: Option<(String, String)>,
    pub(crate) subagent_pane: Option<Entity<ThreadPane>>,
    subagent_events: Option<Subscription>,
    /// An agent's show_panel requests shown so far (`panelRequestGate`).
    pub(crate) panel_gate: threadbear_core::panel_requests::Gate,
    /// Pin requests whose highlights have been fetched once for them, so a
    /// pin unpinned since opens Highlights rather than fetching for good.
    pub(crate) pins_fetched: HashSet<String>,
    /// Why the connection last went, kept while it tries again, so the banner
    /// says what's wrong rather than flicking to "Connecting" on each try
    /// (`connection()`). Cleared when it's back, and by Try again.
    pub(crate) last_drop: Option<(threadbear_client::runner::Disconnect, String)>,
    /// When it stopped being connected (or the app started), for the grace
    /// before the banner says anything about an unexpected drop
    /// (`DROP_GRACE`), and the redraw at its end.
    pub(crate) dropped_at: Option<std::time::Instant>,
    pub(crate) drop_grace_task: Option<Task<()>>,
    /// Start runner is under way (runner_launcher.rs): its button spins.
    pub(crate) starting_runner: bool,
    /// The checkout the runner follows main in (`runner.status`'s
    /// `update.checkout`), where Investigate starts its thread.
    pub(crate) runner_checkout: Option<String>,
    /// The release's checkout on a fork (`updates.upstream.checkout`).
    pub(crate) upstream_checkout: Option<String>,
    /// The last settings change the runner turned down (`settingsError`).
    pub(crate) settings_error: Option<String>,
    /// A runner from before `settings.get`.
    pub(crate) settings_unsupported: bool,
    /// Standing instructions (`instructions.get`), None from a runner without them.
    pub(crate) standing_instructions: Option<threadbear_core::runner_settings::StandingInstructions>,
    /// The runner's version, from hello (`runnerVersion`).
    pub(crate) runner_version: String,
    /// This copy checks for Mike's releases (hello's `updates.upstream`, on
    /// a fork), so Settings offers how often to bring them in.
    pub(crate) upstream_on: bool,
    pub(crate) projects_supported: bool,
    /// Every project's id, title and whether it's archived, for the
    /// header's chips (`projects.list`).
    pub(crate) all_projects: Vec<(String, String, bool)>,
    /// The projects themselves, the page that's open and the views' own
    /// state (projects.rs).
    pub(crate) projects: crate::projects::ProjectsState,

    /// The new-thread page's box and what it's set to (new_thread.rs).
    pub(crate) new_thread: crate::new_thread::NewThreadState,
    /// Other runners' threads and logins (remote.rs).
    pub(crate) remote: crate::remote::RemoteState,
    /// The activity bar's Runners, Updates and Connectors panels
    /// (runners_panel.rs, updates_panel.rs, connectors_panel.rs).
    pub(crate) runners_panel: crate::runners_panel::RunnersPanelState,
    pub(crate) updates_panel: crate::updates_panel::UpdatesPanelState,
    pub(crate) connectors_panel: crate::connectors_panel::ConnectorsPanelState,
    /// Each thread's slash commands, for the composer's palette.
    pub(crate) slash: crate::slash_palette::SlashState,
    /// The Move sheet, while it's up (move_sheet.rs).
    pub(crate) move_sheet: Option<Entity<crate::move_sheet::MoveSheet>>,
    /// The pane beside the thread: Highlights, previews, Patches, Files (highlights.rs).
    pub(crate) side: Entity<SidePane>,
    /// The Threads panel's view, made the first time the dock asks for it
    /// (sidebar.rs, `SidebarView`).
    pub(crate) sidebar_view: Option<Entity<crate::sidebar::SidebarView>>,
    /// The main panel's view, cached as the Threads panel's is (shell.rs).
    pub(crate) main_view: Option<Entity<crate::shell::MainPanelView>>,
    /// Threads open in panels of their own, and the tabs opened beside the
    /// main panel for what it shows (thread_panels.rs).
    pub(crate) thread_panels: crate::thread_panels::ThreadPanels,
    /// Two fingers across the window going back or forward (swipe_nav.rs).
    pub(crate) swipe_nav: crate::swipe_nav::SwipeNav,
    /// Pages on their way to the Archived and Hibernating headings (archive_flight.rs).
    pub(crate) flight: crate::archive_flight::Flights,
    _events: Task<()>,
    _subs: Vec<Subscription>,
}

impl Threadbear {
    pub(crate) fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // gpui-kit starts in its light theme whatever the system says; follow
        // the system's light or dark, and keep following it when it changes,
        // in the system's own window colours (style.rs). Scroll bars show only
        // while scrolling, whatever the system's set to, as the Swift app's
        // transcript and composer have them (`scrollerStyle = .overlay`).
        crate::style::install_window_colours(cx);
        Theme::set_scrollbar_mode(gpui_kit::component::scroll::ScrollbarMode::Scrolling, cx);
        // Light or dark as Settings › General says, else the Mac's, followed
        // as it changes.
        crate::style::set_appearance(crate::style::Appearance::from_name(settings.saved.appearance.as_deref().unwrap_or("system")), window, cx);
        let appearance = window.observe_window_appearance(|window, cx| {
            if crate::style::appearance() == crate::style::Appearance::System {
                Theme::sync_system_appearance(Some(window), cx);
            }
        });
        crate::style::set_zoom_percent(settings.zoom_percent());
        shell::apply_zoom_to_theme(settings.zoom_percent(), cx);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client = Client::new(RunnerConfig::from_env(), ChannelSink(tx));
        RT.spawn(client.clone().run());
        let events = cx.spawn(async move |this, cx| {
            while let Some(seen) = rx.recv().await {
                if this.update(cx, |this, cx| this.on_seen(seen, cx)).is_err() {
                    break;
                }
            }
        });
        let status = client.status();
        let weak_app = cx.entity().downgrade();
        let side_client = client.clone();
        let side = cx.new(|cx| SidePane::new(weak_app, side_client, window, cx));
        crate::viewer::viewer_handle_global(side.downgrade(), cx);
        crate::open_gate::install(cx.entity().downgrade(), cx);
        crate::terminal::install(&cx.entity(), cx);
        // A key with ⌘ held puts the keys shown beside the buttons away.
        crate::key_hints::install(cx.entity().downgrade(), cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search threads"));
        // The filter is worked out at render, so a change only needs a redraw.
        let searching = cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        });
        #[cfg(feature = "automation")]
        crate::automation::serve(cx.entity().downgrade(), window.window_handle(), cx);
        crate::notify::install(cx.entity().downgrade(), window.window_handle(), cx);

        // The dock: the saved layout or the default, and a save whenever it
        // changes (a moment after the last change, as a divider drag is many).
        let layout = shell::make_layout(settings.saved.dock_layout.as_ref());
        let app = cx.entity().downgrade();
        // A reply button's pill presses through here (reply_buttons.rs): the
        // kit draws it inside the reply's text, where there's no app to hand.
        let press_app = app.clone();
        cx.set_global(crate::reply_buttons::PressHook(Rc::new(move |id: String, window: &mut Window, cx: &mut App| {
            press_app.update(cx, |this, cx| this.press_reply_button(id, window, cx)).ok();
        })));
        let dock = cx.new(|_| {
            let mut model = DockModel::new(layout);
            let app = app.clone();
            model.on_change(move |layout: &DockLayout, cx: &mut App| {
                let layout = layout.clone();
                let app = app.clone();
                // Deferred: this runs inside whatever changed the layout,
                // which is often the app itself (a rail button, Reset).
                cx.defer(move |cx| {
                    let _ = app.update(cx, |this, cx| this.dock_layout_changed(layout, cx));
                });
            });
            model
        });
        let content = Rc::new(shell::Panels { app: app.clone() });
        let options = shell::dock_options(app.clone(), cx);
        let dock_view = {
            let model = dock.clone();
            cx.new(|cx| DockView::new(model, content, options, cx))
        };
        let frame = cx.observe_window_bounds(window, |this, window, cx| this.window_frame_changed(window, cx));
        // What's typed and not sent is written as the app quits (drafts.rs).
        let quitting = cx.on_app_quit(|this, cx| {
            // Messages still held for a restarting runner stay held, so the
            // drafts written next can keep them without one also going.
            this.client.stop_holding();
            this.save_drafts(cx);
            // A setting changed in the last moment waits for the throttled
            // save (`save_settings_soon`), which a quit would otherwise lose.
            if std::mem::take(&mut this.settings_dirty) {
                this.settings.save();
            }
            // The activity log's last line (activity.rs).
            crate::activity::quit();
            // A staged build into the installed one's place (app_update.rs).
            this.apply_update_at_quit();
            async {}
        });
        // A thread is read once Mike's looking at it (AppModel.isLooking,
        // which is the app being active, whichever of its windows is in
        // front): coming back reads the open one (appBecameActive). The
        // Settings window says so too (`SettingsWindow::new`).
        crate::thread::set_looking(crate::thread::window_key(window), window.is_window_active());
        let looking = cx.observe_window_activation(window, |this, window, cx| this.looking_changed(crate::thread::window_key(window), window.is_window_active(), cx));
        // A pin an agent asked to show may only be in Highlights once they've
        // been fetched: their coming is a moment to try it again.
        let side_changed = cx.observe(&side, |this: &mut Threadbear, _, cx| this.serve_panel_request(cx));
        // Everything the menus say is the app's state, which notifies when it
        // changes (the dock's layout too: `dock_layout_changed`).
        // Its 20-second tick notifies too (SidebarState), so the badge's
        // count follows a snooze running out as the list does.
        let menus = cx.observe(&cx.entity(), |this: &mut Threadbear, _, _| {
            this.menus_stale = true;
            this.inbox_count_cache.set(None);
            this.projects.pane_cache.take();
        });
        // Tabs opened beside the main panel for what it showed go when it
        // moves on (thread_panels.rs, `mainContextChanged`).
        let main_context = cx.observe(&cx.entity(), |this: &mut Threadbear, _, cx| this.main_context_maybe_changed(cx));

        let mut sidebar = crate::sidebar::SidebarState::new(cx);
        sidebar.order = settings.inbox_order();
        let scheduled = crate::scheduled::ScheduledState::new(settings.saved.collapsed_schedule_sections.as_deref());
        let usage = crate::usage::UsageState::new(settings.saved.usage_chart_range.as_deref(), settings.saved.usage_show_model_weeks);
        let settings_open_rows = settings.saved.projects_open_rows.clone();
        let folded: HashSet<&'static str> = match &settings.saved.collapsed_sections {
            // The section keys are the Swift enum's raw values.
            Some(saved) => ["starred", "inbox", "running", "hibernating", "archived"].into_iter().filter(|k| saved.iter().any(|s| s == k)).collect(),
            None => HashSet::from(["hibernating", "archived"]),
        };
        let mut this = Threadbear {
            client,
            status,
            threads: HashMap::new(),
            list_error: None,
            selected: None,
            pane: None,
            pane_events: None,
            kept: Default::default(),
            search,
            folded,
            composers: HashMap::new(),
            drafts: crate::drafts::Drafts::load(&threadbear_client::config::RunnerConfig::from_env().home),
            logins: RunnerLogins::default(),
            sidebar,
            root_focus: cx.focus_handle(),
            settings,
            settings_dirty: false,
            settings_last_save_ms: 0,
            settings_save_task: None,
            dock,
            dock_view,
            #[cfg(feature = "frame-stats")]
            frame_rate: None,
            hidden_panes: Vec::new(),
            mode: SidebarMode::Inbox,
            history: NavigationHistory::default(),
            shortcuts: ShortcutBindings::default(),
            bound_keys: HashMap::new(),
            shortcuts_watch: None,
            shortcuts_path: None,
            menu_signature: String::new(),
            menus_stale: true,
            failed_on_account: HashMap::new(),
            limit_pick: HashMap::new(),
            inbox_count_cache: Default::default(),
            last_window_title: String::new(),
            archived_note: None,
            inbox_end_note: None,
            import_note: None,
            updater: Default::default(),
            note_counter: 0,
            note_task: None,
            archived_task: None,
            toast_hovered: false,
            error_message: None,
            connection_error: None,
            busy_notes: Vec::new(),
            done_note: None,
            done_task: None,
            dismissed_cli_failures: HashSet::new(),
            restart_status_task: None,
            restart_expiry: None,
            dropped_at: Some(std::time::Instant::now()),
            drop_grace_task: None,
            creating_investigation: false,
            renaming: None,
            rename_field: None,
            share_popover: None,
            share_busy: false,
            chip_busy: Default::default(),
            continuing_refusal: None,
            website_url: None,
            schedules_supported: false,
            scheduled,
            schedule_editor: None,
            shows_home: false,
            restoring_place: false,
            librarian: crate::librarian::LibrarianState::new(),
            librarian_pokes: crate::home_page::Pokes::new(crate::home_page::Art::Librarian),
            usage,
            runner_settings: None,
            small_jobs: vec!["claude".into()],
            settings_window: None,
            main_window: window.window_handle(),
            pending_open: None,
            pending_restore: None,
            waiting_link: None,
            dock_badge: None,
            panel_gate: Default::default(),
            subagent: None,
            subagent_pane: None,
            subagent_events: None,
            pins_fetched: HashSet::new(),
            last_drop: None,
            starting_runner: false,
            runner_checkout: None,
            upstream_checkout: None,
            settings_error: None,
            settings_unsupported: false,
            standing_instructions: None,
            runner_version: String::new(),
            upstream_on: false,
            projects_supported: false,
            all_projects: Vec::new(),
            projects: crate::projects::ProjectsState::new(settings_open_rows.as_deref()),
            new_thread: crate::new_thread::NewThreadState::new(),
            remote: Default::default(),
            runners_panel: Default::default(),
            updates_panel: Default::default(),
            connectors_panel: Default::default(),
            slash: Default::default(),
            move_sheet: None,
            side,
            sidebar_view: None,
            main_view: None,
            thread_panels: Default::default(),
            swipe_nav: Default::default(),
            flight: Default::default(),
            _events: events,
            _subs: vec![appearance, searching, frame, menus, looking, quitting, side_changed, main_context],
        };
        // Records nothing unless it's switched on in Settings (activity.rs).
        let showing = crate::activity::showing(this.dock.read(cx).layout());
        crate::activity::install(cx.entity().downgrade(), window.window_handle(), &RunnerConfig::from_env().home, this.settings.saved.activity_log == Some(true), showing, cx);
        // Following main, and back to where it was after a restart the app
        // made itself for an update (app_update.rs).
        this.start_updater(cx);
        this.restore_after_update(cx);
        // The window starts on the Inbox with nothing open (`noteNavigation`
        // on launch), so Back from the first thread comes back here.
        this.note_navigation();
        this.watch_shortcuts(cx);
        this
    }

    /// A reply button's pill was clicked (AppModel.pressReplyButton). Not
    /// pressed yet: presses it, "…" until the runner answers, and a second
    /// click meanwhile does nothing. Pressed: opens the thread it started or
    /// messaged rather than pressing again.
    pub(crate) fn press_reply_button(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some((thread_id, button)) = crate::reply_buttons::known(&id) else { return };
        // Only from the transcript of the thread that made it: ids are the
        // runner's per thread, and a link copied into another thread's reply
        // mustn't press this one. A thread in a panel of its own has its
        // transcript there (thread_panels.rs).
        if self.selected.as_deref() != Some(thread_id.as_str()) && self.thread_panel_pane(&thread_id).is_none() {
            self.show_error("That button belongs to another thread, so it can't be pressed from here.".into(), cx);
            return;
        }
        if let Some(pressed) = &button.pressed {
            if self.threads.contains_key(&pressed.thread_id) {
                self.open(pressed.thread_id.clone(), window, cx);
            } else {
                let what = if button.send.is_some() { "messaged" } else { "started" };
                self.show_error(format!("The thread that button {what} isn't in your sidebar, so it can't open here. It was deleted, or it's archived on another machine."), cx);
            }
            return;
        }
        if !crate::reply_buttons::set_pressing(&id, true) {
            return;
        }
        self.redraw_pane(cx);
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "buttons.press", json!({ "threadId": thread_id, "buttonId": id })).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        if let Some(button) = threadbear_core::reply_buttons::ReplyButton::decode(&v["button"]) {
                            crate::reply_buttons::receive([button], &thread_id);
                        }
                    }
                    Err(e) => this.show_error(e.message, cx),
                }
                crate::reply_buttons::set_pressing(&id, false);
                this.redraw_pane(cx);
            })
            .ok();
        })
        .detach();
    }

    /// The open thread draws again: its pills read the buttons as they draw.
    fn redraw_pane(&self, cx: &mut Context<Self>) {
        for pane in self.pane.iter().chain(self.thread_panels.panes()) {
            pane.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// One of the app's windows came to the front or went (`active`, as the
    /// window itself has it, set before its observers run): with any in
    /// front, the open thread is read.
    pub(crate) fn looking_changed(&mut self, window: u64, active: bool, cx: &mut Context<Self>) {
        crate::thread::set_looking(window, active);
        // ⌘-Tab away lets ⌘ go in the other app, so this one never hears it.
        if !active {
            self.hints_reset(cx);
            // A swipe's lift goes to the app in front now (swipe_nav.rs, row_swipe.rs).
            self.cancel_page_swipe(cx);
            self.cancel_row_swipe(cx);
        }
        crate::activity::window_active(window, active, cx);
        if let (true, Some(pane)) = (crate::thread::is_looking(), self.pane.clone()) {
            pane.read(cx).mark_seen();
        }
        // And the threads in panels of their own that are in front.
        self.mark_thread_panels_seen(cx);
        self.serve_panel_request(cx);
    }

    /// An agent's `show_panel` on the open thread, once Mike's looking at it
    /// (`servePanelRequest`): the panel brought up where it asked, or to the
    /// front where it is, a pin opened in the pane, and the runner told.
    pub(crate) fn serve_panel_request(&mut self, cx: &mut Context<Self>) {
        use threadbear_core::panel_requests::{self as requests, RequestedPanel, RequestedPlace};
        if self.shows_home {
            return;
        }
        let Some(thread) = self.open_thread().cloned() else { return };
        let Some(request) = self.panel_gate.take(Some(&thread), crate::thread::is_looking()) else { return };
        if let Some(pin) = request.highlight.clone() {
            let item = self.side.read(cx).highlights.get(&thread.id).and_then(|h| h.highlights.iter().find(|x| x.id == pin).cloned());
            if item.is_none() && self.pins_fetched.insert(request.id.clone()) {
                // Fetched first; their coming tries it again.
                self.panel_gate.release(&request.id);
                let id = thread.id.clone();
                self.side.update(cx, |side, cx| side.refresh_highlights(id, std::time::Duration::ZERO, cx));
                return;
            }
            self.show_requested(&RequestedPanel::Section(requests::HIGHLIGHTS.to_owned()), &RequestedPlace::Usual, &thread, cx);
            // Only the pane: an agent showing a file never starts its app.
            // One unpinned since still opens Highlights.
            if let Some((path, title)) = item.and_then(|i| i.path.clone().map(|p| (p, i.title.clone()))) {
                let (shown, side) = (thread.clone(), self.side.clone());
                // Once this update is over: the pane reads the app to know its
                // thread, which GPUI refuses while the app is the one being
                // updated. Opening a thread whose agent had shown a pin
                // crashed the app (Mike, 12 Nov).
                cx.defer(move |cx| side.update(cx, |side, cx| side.preview_path(&path, Some(title), &shown, cx)));
            }
            self.tell_panel_shown(&request, &thread);
            return;
        }
        // One the thread hasn't got is left for the runner to clear with
        // Mike's next message.
        let Some(panel) = requests::panel(&request, &thread) else { return };
        if request.reload == Some(true) {
            crate::web_panels::reload_requested(&thread, &request.panel, cx);
            // A file opened over the panel from its page would still cover
            // what the agent changed.
            let (thread_id, from) = (thread.id.clone(), request.panel.clone());
            self.side.update(cx, |side, cx| {
                if side.previews.get(&thread_id).is_some_and(|pv| pv.from == from) {
                    side.close_preview(&thread_id, cx);
                }
            });
        }
        self.show_requested(&panel, &requests::place(&request), &thread, cx);
        self.tell_panel_shown(&request, &thread);
    }

    /// Opens the panel at `place`, or brings it to the front where it is. A
    /// pane section becomes the thread's chosen one, as a click on its rail
    /// button makes it (`showRequested`).
    fn show_requested(&mut self, panel: &threadbear_core::panel_requests::RequestedPanel, place: &threadbear_core::panel_requests::RequestedPlace, thread: &ThreadSummary, cx: &mut Context<Self>) {
        use threadbear_core::panel_requests::{RequestedPanel, RequestedPlace};
        use threadbear_dock::{DockDrop, DockEdge};
        let id = match panel {
            RequestedPanel::Section(section) => {
                let (thread_id, section) = (thread.id.clone(), section.clone());
                self.side.update(cx, |side, cx| side.choose_section(&thread_id, &section, cx));
                section
            }
            RequestedPanel::Usage => shell::USAGE.to_owned(),
            RequestedPanel::Updates => shell::UPDATES.to_owned(),
            RequestedPanel::Connectors => shell::CONNECTORS.to_owned(),
        };
        if self.dock.read(cx).layout().contains(&id) {
            self.dock.update(cx, |m, cx| m.show(&id, &[], DockDrop::window(DockEdge::Right), cx));
            return;
        }
        match place {
            RequestedPlace::Usual => match panel {
                RequestedPanel::Section(_) => self.show_pane(&id, cx),
                _ => self.toggle_under_threads(&id, cx),
            },
            // Beside the main panel, for what it shows: it goes when the main
            // panel moves on, unless it's dragged away (thread_panels.rs).
            RequestedPlace::Tab => self.open_main_tab(&id, cx),
            RequestedPlace::Side(side) => {
                let edge = match side.as_str() {
                    "left" => DockEdge::Left,
                    "top" => DockEdge::Top,
                    "bottom" => DockEdge::Bottom,
                    _ => DockEdge::Right,
                };
                self.dock.update(cx, |m, cx| m.open(&id, DockDrop::window(edge), cx));
            }
            RequestedPlace::Window => self.dock.update(cx, |m, cx| m.open_in_new_window(&id, threadbear_dock::Size::new(520.0, 640.0), cx)),
        }
    }

    /// `threads.panelShown`, which clears the request on the runner.
    fn tell_panel_shown(&self, request: &threadbear_core::panel_requests::PanelRequest, thread: &ThreadSummary) {
        let client = self.client.clone();
        let params = json!({ "threadId": thread.id, "requestId": request.id });
        RT.spawn(async move { client.call("threads.panelShown", params).await });
    }

    fn on_seen(&mut self, seen: Seen, cx: &mut Context<Self>) {
        match seen {
            Seen::Status(status) => {
                let connected = matches!(status, Status::Connected { .. });
                if let Status::Connected { hello, .. } = &status {
                    self.logins = RunnerLogins::from_hello(hello);
                    self.remote.experiments = hello["experiments"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_owned)).collect();
                    self.runner_version = hello["runnerVersion"].as_str().unwrap_or("").to_owned();
                    self.upstream_on = hello["updates"]["upstream"]["state"].as_str().is_some_and(|s| s != "off");
                    self.upstream_checkout = hello["updates"]["upstream"]["checkout"].as_str().filter(|c| !c.is_empty()).map(str::to_owned);
                    // A runner from before `providers` has Claude's built in.
                    if let Some(providers) = hello["providers"].as_array() {
                        self.small_jobs = providers.iter().filter(|p| p["smallJobs"].as_bool() == Some(true)).filter_map(|p| p["id"].as_str().map(str::to_owned)).collect();
                    }
                    // The runner's own update and the CLIs, for the Updates panel.
                    self.updates_from_hello(hello);
                }
                match &status {
                    Status::Connected { .. } => {
                        self.last_drop = None;
                        self.dropped_at = None;
                        self.drop_grace_task = None;
                        self.clear_connection_error();
                    }
                    Status::Disconnected { kind, reason, .. } => {
                        self.last_drop = Some((*kind, reason.clone()));
                        self.watch_restart_expiry(cx);
                        self.start_drop_grace(cx);
                    }
                    Status::Connecting { .. } => self.start_drop_grace(cx),
                }
                self.status = status;
                // Each time it's connected, the whole list again, and the
                // open thread read again: thread.updated only says what changed.
                if connected {
                    self.load_list(cx);
                    self.refresh_restart(cx);
                    self.load_projects(cx);
                    self.load_runner_settings(cx);
                    self.load_schedules(cx);
                    self.load_remote(cx);
                    self.load_clis(cx);
                    self.load_connectors(cx);
                    self.side.update(cx, |side, cx| side.connected(cx));
                    for pane in self.pane.iter().chain(self.thread_panels.panes()) {
                        pane.update(cx, |p, cx| p.load(cx));
                    }
                    // Kept ones catch up from their last seq when next shown.
                    for pane in self.kept.panes() {
                        pane.update(cx, |p, _| p.connection_back());
                    }
                }
            }
            Seen::Note(method, params) => match method.as_str() {
                "thread.updated" => {
                    if let Ok(mut t) = serde_json::from_value::<ThreadSummary>(params["thread"].clone()) {
                        // Another runner's thread from this Mac's runner (the
                        // relay) stays marked as that runner's.
                        t.runner_id = self.threads.get(&t.id).and_then(|old| old.runner_id.clone());
                        if let Some(pane) = &self.pane {
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        if let Some(pane) = &self.subagent_pane {
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        if let Some(pane) = self.thread_panel_pane(&t.id) {
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        if let Some(pane) = self.kept.get(&t.id, cx) {
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        let before = self.threads.insert(t.id.clone(), t.clone());
                        self.note_failed_account(before.as_ref(), &t);
                        self.thread_changed_outside(before.as_ref(), &t, cx);
                        if t.panel_request.is_some() && self.selected.as_deref() == Some(t.id.as_str()) {
                            self.serve_panel_request(cx);
                        }
                        crate::terminal::thread_updated(&t, cx);
                        // A thread a pending restart waits for may have just
                        // finished: the banner's count and list follow.
                        if self.client.restart().is_pending() {
                            self.refresh_restart_soon(cx);
                        }
                    }
                }
                "thread.deleted" => {
                    let id = params["threadId"].as_str().unwrap_or("");
                    crate::terminal::thread_deleted(id, cx);
                    self.kept.forget(id, cx);
                    // Its panel of its own goes with it (`forget`).
                    self.close_thread_panel(id, cx);
                    self.threads.remove(id);
                    self.failed_on_account.remove(id);
                    self.refresh_dock_badge();
                }
                "runner.notice" => {
                    // A restart notice moves the quiet banner along (the
                    // connection has already noted it, restart.rs), and asks
                    // which threads it waits for; other warnings and errors
                    // go in the error bar (`notice`).
                    let message = params["message"].as_str().unwrap_or("");
                    if threadbear_client::restart::RunnerRestart::parse(message).is_some() {
                        self.refresh_restart(cx);
                        self.watch_restart_expiry(cx);
                    } else if params["level"].as_str().is_some_and(|l| l != "info") && !message.is_empty() {
                        self.show_error(message.to_owned(), cx);
                    }
                }
                "librarian.updated" => self.librarian.status = Some(threadbear_core::librarian::Status::decode(&params["librarian"])),
                "settings.updated" => self.runner_settings = Some(params["settings"].clone()),
                // A login appearing or going, its models once asked, its
                // usage; and the pools after a save: both replace `hello`'s.
                "accounts.updated" => self.logins.set_accounts(&params["accounts"]),
                "pools.updated" => self.logins.set_pools(&params["pools"]),
                "instructions.updated" => self.standing_instructions = Some(threadbear_core::runner_settings::StandingInstructions::decode(&params["instructions"])),
                "projects.updated" => {
                    self.librarian.projects_made = crate::librarian::projects_made(&params["projects"]);
                    self.sidebar.project_icons = crate::sidebar::project_icons(&params["projects"]);
                    self.sidebar.projects = crate::sidebar::project_briefs(&params["projects"]);
                    self.all_projects = project_list(&params["projects"]);
                    self.receive_projects(&params["projects"]);
                    self.side.update(cx, |side, cx| side.projects_updated(&params["projects"], cx));
                }
                "schedules.updated" => {
                    // The whole list, in order.
                    self.schedules_supported = true;
                    self.set_schedules(&params["schedules"]);
                    self.side.update(cx, |side, cx| side.schedules_updated(&params["schedules"], cx));
                }
                "remoteThreads.updated" => self.receive_remote(&params, cx),
                "clis.updated" => self.clis_updated(&params),
                "runner.updates" => self.runner_updates_changed(&params),
                "connectors.updated" => self.connectors_updated(&params),
                "thread.event" => {
                    // An open review window hears its review change whichever
                    // thread is open here (review_window.rs).
                    crate::review_window::receive_event(&params, cx);
                    // Each pane takes its own thread's events only: the open
                    // ones, and those kept loaded, which stay subscribed.
                    let id = params["threadId"].as_str().unwrap_or("");
                    for pane in self.pane.iter().chain(self.thread_panels.panes()) {
                        pane.update(cx, |p, cx| p.event(&params, cx));
                    }
                    if let Some(pane) = self.kept.get(id, cx) {
                        pane.update(cx, |p, cx| p.event(&params, cx));
                    }
                }
                _ => {}
            },
        }
        cx.notify();
    }

    /// What a summary changing says outside the thread itself (`apply`): a
    /// notification if it's just come to need Mike and he isn't looking at
    /// it, the ding if it's just finished, and the Dock badge's count. Not
    /// another runner's thread, nor a scripted app's (notify.rs).
    fn thread_changed_outside(&mut self, before: Option<&ThreadSummary>, t: &ThreadSummary, cx: &mut Context<Self>) {
        let looking = crate::thread::is_looking() && self.selected.as_deref() == Some(t.id.as_str());
        if threadbear_core::inbox::should_notify(before, t, looking) {
            crate::notify::post(t, cx);
        }
        if t.runner_id.is_none() && crate::notify::enabled() {
            crate::ding::thread_changed(before.map(|b| b.status.as_str()), &t.status, &self.settings);
        }
        self.refresh_dock_badge();
    }

    /// The Dock badge: the threads waiting on Mike (`refreshDockBadge`).
    pub(crate) fn refresh_dock_badge(&mut self) {
        let label = threadbear_core::inbox::dock_badge(threadbear_core::inbox::needs_you_count(self.threads.values(), now_ms()));
        if label != self.dock_badge {
            crate::notify::set_badge(label.as_deref());
            self.dock_badge = label;
        }
    }

    /// A thread asked for from outside the window (a notification's click):
    /// the Inbox, with it open, or once the list has loaded.
    pub(crate) fn open_from_outside(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        window.activate_window();
        if !self.sidebar.loaded {
            self.pending_open = Some(id);
            return;
        }
        self.set_mode(SidebarMode::Inbox, cx);
        self.open(id, window, cx);
    }

    pub(crate) fn open_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.pending_restore.take() {
            // Back where it was after an update's restart: not
            // `open_from_outside`, which brings the window in front (taking
            // it from the app Mike's in) and goes to the Inbox, losing the
            // list that was showing (Codex's review, 12 Nov).
            let (me, window) = (cx.entity().downgrade(), self.main_window);
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        me.update(cx, |this, cx| this.open(id, window, cx)).ok();
                    })
                    .ok();
            });
        }
        let Some(id) = self.pending_open.take() else { return };
        let (me, window) = (cx.entity().downgrade(), self.main_window);
        // Not from inside this update: opening needs the window, and the
        // window's update would come back to this entity.
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    me.update(cx, |this, cx| this.open_from_outside(id, window, cx)).ok();
                })
                .ok();
        });
    }

    fn load_list(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.list", json!({ "include": ["inbox", "snoozed", "archived"] })).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        let list: Vec<ThreadSummary> = serde_json::from_value(v["threads"].clone()).unwrap_or_default();
                        let before = std::mem::take(&mut this.threads);
                        this.threads = list.into_iter().map(|t| (t.id.clone(), t)).collect();
                        // Open panes take the summaries as they are now: on
                        // a reconnect this list is the only word of what
                        // changed while it was away (a loose-ends check that
                        // finished then isn't sent again), and each pane's
                        // reload reads its own summary (Codex's review, 12 Nov).
                        let panes: Vec<_> = this.pane.iter().chain(this.subagent_pane.iter()).chain(this.thread_panels.panes()).chain(this.kept.panes()).cloned().collect();
                        for pane in panes {
                            let Some(mut t) = this.threads.get(&pane.read(cx).summary.id).cloned() else { continue };
                            t.runner_id = pane.read(cx).summary.runner_id.clone();
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        crate::terminal::list_loaded(&this.threads, cx);
                        this.note_failed_accounts(&before);
                        // Other runners' rows stay listed beside these (and
                        // kept threads no longer listed go, remote.rs).
                        this.apply_remote_index(cx);
                        this.list_error = None;
                        this.sidebar.loaded = true;
                        this.refresh_dock_badge();
                        this.open_pending(cx);
                        this.try_waiting_link(cx);
                    }
                    Err(e) => this.list_error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether the runner has schedules at all, and how many are active
    /// (`schedulesSupported`, `activeScheduleCount`), for the activity bar.
    pub(crate) fn load_schedules(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "schedules.list", json!({})).await;
            this.update(cx, |this, cx| {
                if let Ok(v) = res {
                    this.schedules_supported = true;
                    this.set_schedules(&v["schedules"]);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The runner's settings (`settings.get`), for the pages that show some
    /// of them (the Librarian's Runs on), and Settings once it's here.
    pub(crate) fn load_runner_settings(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "settings.get", json!({})).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        this.runner_settings = Some(v["settings"].clone());
                        this.settings_unsupported = false;
                    }
                    // A runner from before settings.
                    Err(e) if e.code == -32601 => {
                        this.settings_unsupported = true;
                        this.runner_settings = None;
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `updateSettings`: shown at once; the runner's answer (or its error,
    /// which puts it back) has the last word. `change` is what
    /// `settings.update` takes.
    pub(crate) fn update_runner_settings(&mut self, change: Value, cx: &mut Context<Self>) {
        let before = self.runner_settings.clone();
        if let Some(settings) = self.runner_settings.as_mut() {
            merge_json(settings, &change);
        }
        let guess = self.runner_settings.clone();
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "settings.update", change).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        this.runner_settings = Some(v["settings"].clone());
                        this.settings_error = None;
                    }
                    Err(e) => {
                        // Put back only if nothing newer has come since
                        // (`settings.updated` from another window), and ask
                        // again either way: the runner has the last word.
                        if this.runner_settings == guess {
                            this.runner_settings = before;
                        }
                        this.load_runner_settings(cx);
                        // Under the form in Settings (`SettingsErrorRow`).
                        this.settings_error = Some(e.message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Settings › General › Appearance: this app's own, kept in its settings
    /// file (`AppAppearance`).
    pub(crate) fn set_appearance(&mut self, appearance: crate::style::Appearance, window: &mut Window, cx: &mut Context<Self>) {
        crate::style::set_appearance(appearance, window, cx);
        self.settings.saved.appearance = (appearance != crate::style::Appearance::System).then(|| appearance.name().to_owned());
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// The providers whose logins can run small jobs (`smallJobs`).
    pub(crate) fn small_job_providers(&self) -> Vec<String> {
        self.small_jobs.clone()
    }

    pub(crate) fn open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_with(id, None, window, cx);
    }

    /// `open`, with the pane of a thread brought back from a panel of its own
    /// (thread_panels.rs), which goes on reading it rather than reading it again.
    pub(crate) fn open_with(&mut self, id: String, moved: Option<Entity<ThreadPane>>, window: &mut Window, cx: &mut Context<Self>) {
        // A thread (its own row, say) in place of one of its sub-agents.
        self.close_subagent(cx);
        // A thread open in a panel of its own is shown there.
        if moved.is_none() && self.open_in_its_panel(&id, cx) {
            return;
        }
        if self.selected.as_deref() == Some(id.as_str()) {
            // Open already, behind a page (a schedule's target was clicked,
            // or a project's page): the page goes.
            if self.shows_home || self.projects.open.is_some() {
                self.shows_home = false;
                self.projects.open = None;
                self.note_navigation();
                cx.notify();
            }
            return;
        }
        let Some(summary) = self.threads.get(&id).cloned() else { return };
        // Another runner's thread: its logins fill the pickers once they've
        // come (`openClient`'s `loadRelayLogins`).
        if let Some(runner) = summary.runner_id.clone() {
            self.load_remote_logins(runner, cx);
        }
        // Kept loaded since it was last open: shown as it was at once, and
        // caught up if it needs to be (pane_cache.rs). Taken out before the
        // open one goes in, which could otherwise push it out. A pane brought
        // from a panel is the thread's one, and a kept one can't be too.
        let kept = self.kept.take(&id, cx).filter(|_| moved.is_none());
        // The thread that was open stays loaded, for switching back.
        self.leave_pane(cx);
        let client = self.client.clone();
        let composer = self.composer_for(&id, window, cx);
        self.selected = Some(id);
        self.shows_home = false;
        self.projects.open = None;
        let pane = match (moved, kept) {
            (Some(pane), _) => pane,
            (None, Some(pane)) => {
                pane.update(cx, |p, cx| {
                    p.summary_changed(&summary, cx);
                    p.shown_again(cx);
                });
                pane
            }
            (None, None) => cx.new(|cx| ThreadPane::new(client, summary, composer.clone(), cx)),
        };
        composer.update(cx, |c, cx| {
            c.attach(pane.downgrade());
            c.focus(window, cx);
            crate::web_panels::keyboard_to_app(window);
            cx.notify();
        });
        // A thread: link in the transcript opens that thread.
        self.pane_events = Some(cx.subscribe_in(&pane, window, |this, _, event: &crate::rows::OpenThreadRequest, window, cx| this.open_thread_link(&event.0, window, cx)));
        self.pane = Some(pane);
        self.note_navigation();
        cx.notify();
    }

    /// A `thread:` link (its id or a unique start of one) opens that thread.
    /// One that matches none, or several, beeps and says why: a beep alone
    /// left Mike clicking a link to a thread archived on bruce (another
    /// machine's archived threads aren't listed here) and wondering why
    /// nothing opened (6 Nov; `openThreadLink`).
    pub(crate) fn open_thread_link(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let matches: Vec<String> = self.threads.keys().filter(|k| *k == id || k.starts_with(id)).cloned().collect();
        if let [only] = matches.as_slice() {
            // In the Inbox view, its section unfolded and its row in view,
            // as Swift's `openThread` has it.
            self.reveal_thread(only, cx);
            return self.open(only.clone(), window, cx);
        }
        crate::composer_answers::beep();
        self.show_error(threadbear_core::thread_link::unopened_link_message(matches.len()), cx);
    }

    /// The pane holding a thread now, if one does: the main panel's, the
    /// thread's own panel, or one kept loaded. Not a sub-agent's, which shows
    /// another transcript.
    pub(crate) fn pane_for(&self, id: &str, cx: &App) -> Option<Entity<ThreadPane>> {
        let main = self.pane.clone().filter(|p| {
            let p = p.read(cx);
            p.summary.id == id && p.subagent_id.is_none()
        });
        // A thread kept loaded (pane_cache.rs) hears too, so it's right
        // when it's shown again.
        main.or_else(|| self.thread_panel_pane(id)).or_else(|| self.kept.get(id, cx))
    }

    /// A thread's message box, kept while others are open so what's typed
    /// stays with it, and made (with its words from before a quit) the first
    /// time it's wanted.
    pub(crate) fn composer_for(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<Composer> {
        if let Some(composer) = self.composers.get(id) {
            return composer.clone();
        }
        let (client, app) = (self.client.clone(), cx.entity().downgrade());
        let composer = cx.new(|cx| Composer::new(Some(id.to_owned()), client, app, window, cx));
        self.composers.insert(id.to_owned(), composer.clone());
        self.restore_saved_draft(id, &composer, cx);
        composer
    }

    /// Nothing open: the new-thread screen's place in the history. For going
    /// Back to it (`show(place)` with no thread).
    /// A sub-agent's transcript in the main panel, with its thread selected
    /// under it (`selectSubagent`): Back, or opening a thread, puts the
    /// thread back.
    pub(crate) fn open_subagent(&mut self, thread_id: String, subagent_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(thread_id.as_str()) || self.shows_home {
            self.open(thread_id.clone(), window, cx);
        }
        self.close_subagent(cx);
        let (Some(summary), Some(composer)) = (self.threads.get(&thread_id).cloned(), self.composers.get(&thread_id).cloned()) else { return };
        let client = self.client.clone();
        let pane = cx.new(|cx| ThreadPane::new_subagent(client, summary, subagent_id.clone(), composer, cx));
        // A thread: link in its transcript opens that thread, as in a thread's.
        self.subagent_events = Some(cx.subscribe_in(&pane, window, |this, _, event: &crate::rows::OpenThreadRequest, window, cx| this.open_thread_link(&event.0, window, cx)));
        self.subagent = Some((thread_id, subagent_id));
        self.subagent_pane = Some(pane);
        cx.notify();
    }

    /// Back to the thread from one of its sub-agents.
    pub(crate) fn close_subagent(&mut self, cx: &mut Context<Self>) {
        if self.subagent.take().is_some() {
            self.subagent_pane = None;
            self.subagent_events = None;
            cx.notify();
        }
    }

    pub(crate) fn select_none(&mut self, cx: &mut Context<Self>) {
        self.close_subagent(cx);
        self.leave_pane(cx);
        self.selected = None;
        self.shows_home = false;
        self.projects.open = None;
        self.note_navigation();
        cx.notify();
    }

    /// The main panel's thread stops being shown there: it's kept loaded
    /// (pane_cache.rs), still subscribed, for switching back to it.
    pub(crate) fn leave_pane(&mut self, cx: &mut Context<Self>) {
        self.pane_events = None;
        if let Some(old) = self.pane.take() {
            self.kept.keep(old, cx);
        }
    }

    /// `threads.update` for the open thread, with the summary changed here
    /// first: anything clicked shows at once, and the runner's answer (or its
    /// thread.updated) puts it right.
    pub(crate) fn update_open(&mut self, change: impl FnOnce(&mut ThreadSummary), params: Value, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(id) = self.selected.clone() else { return Err("no thread open".into()) };
        let before = self.threads.get(&id).cloned();
        if let Some(t) = self.threads.get_mut(&id) {
            change(t);
        }
        // A folder, login or pool change isn't guessed, so its chip spins.
        let busy = if params.get("cwd").is_some() {
            Some(format!("movingFolder/{id}"))
        } else if params.get("accountId").is_some() || params.get("poolId").is_some() {
            Some(format!("moving/{id}"))
        } else {
            None
        };
        if let Some(key) = &busy {
            self.chip_busy.insert(key.clone());
        }
        let mut params = params;
        params["threadId"] = json!(id);
        let client = self.client.clone();
        let remote = before.as_ref().is_some_and(|t| t.runner_id.is_some());
        let going = crate::call_for_thread(client, remote, "threads.update", params);
        cx.spawn(async move |this, cx| {
            let res = going.await;
            this.update(cx, |this, cx| {
                if let Some(key) = &busy {
                    this.chip_busy.remove(key);
                }
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
        Ok(())
    }

    #[cfg(feature = "automation")]
    pub(crate) fn snooze_open(&mut self, seconds: i64, cx: &mut Context<Self>) -> Result<(), String> {
        let until = iso_time(now_ms() + seconds * 1000);
        let local = until.clone();
        self.update_open(move |t| t.snoozed_until = Some(local), json!({ "snoozedUntil": until }), cx)
    }

    pub(crate) fn stop_open(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.stop(cx));
        }
    }

    #[cfg(feature = "automation")]
    pub(crate) fn composer(&self, cx: &App) -> Option<Entity<Composer>> {
        // The open thread's box, or the new-thread page's.
        self.pane.as_ref().map(|p| p.read(cx).composer.clone()).or_else(|| self.new_thread.composer.clone())
    }

    #[cfg(feature = "automation")]
    pub(crate) fn set_row_open(&mut self, row: &str, open: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(pane) = &self.pane else { return Err("no thread open".into()) };
        pane.update(cx, |p, cx| p.set_row_open(row, open, cx))
    }

    #[cfg(feature = "automation")]
    pub(crate) fn load_all_events(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.load_all_events(cx));
        }
    }

    /// Runs `f` on the open pane, for the automation commands.
    #[cfg(feature = "automation")]
    pub(crate) fn update_pane<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut ThreadPane, &mut Context<ThreadPane>) -> R) -> Result<R, String> {
        let Some(pane) = &self.pane else { return Err("no thread open".into()) };
        Ok(pane.update(cx, f))
    }

    /// A thread named by the start of its id, as the Swift app's automation has it.
    #[cfg(feature = "automation")]
    pub(crate) fn thread_id_from(&self, start: &str) -> Result<String, String> {
        let mut found = self.threads.keys().filter(|id| id.starts_with(start));
        match (found.next(), found.next()) {
            (Some(id), None) => Ok(id.clone()),
            (None, _) => Err(format!("no thread starting {start:?}")),
            _ => Err(format!("more than one thread starts {start:?}")),
        }
    }

    /// The window as data, in the Swift app's shape (app/Sources/Threadbear/AppDump.swift).
    #[cfg(feature = "automation")]
    pub(crate) fn dump(&self, cx: &App) -> Value {
        // The Swift dump's names (`connectionName`).
        let connection = match self.connection() {
            Connection::Connected => "connected",
            Connection::Connecting => "connecting",
            Connection::Down(_) => "down",
            Connection::Lost(_) => "lost",
            Connection::Unauthorized(_) => "unauthorized",
        };
        let pane = self.pane.as_ref().map(|p| p.read(cx));
        let dock = self.dock.read(cx).layout().summary().replace("threadbear.", "");
        let mut sidebar = self.sidebar_dump(cx);
        sidebar["mode"] = json!(self.mode.name());
        // The Scheduled view in the thread shape, as the Swift app's dump has it.
        if self.mode == SidebarMode::Scheduled {
            sidebar["sections"] = self.scheduled_dump()["sections"].clone();
        }
        let (librarian_sections, librarian_state) = self.librarian_dump();
        if self.mode == SidebarMode::Librarian {
            sidebar["sections"] = librarian_sections;
        }
        if self.mode == SidebarMode::Projects {
            sidebar["sections"] = self.projects_sidebar_dump();
        }
        let mut dump = json!({
            "connection": connection,
            "selection": self.selection_dump(),
            "sidebar": sidebar,
            "composer": self.dump_composer(cx),
            "newThread": crate::new_thread::page_dump(self, cx),
            "palette": self.composer(cx).map_or(Value::Null, |c| c.read(cx).palette_dump(cx)),
            "moveSheet": self.move_sheet.as_ref().map_or(Value::Null, |s| s.read(cx).dump(cx)),
            "scheduleEditor": self.schedule_editor.as_ref().map_or(Value::Null, |e| e.read(cx).dump(cx)),
            // The Settings window while it's open, and what it shows.
            "settingsWindow": self.settings_window.as_ref().map_or(Value::Null, |(_, view)| view.read(cx).dump(cx)),
            // The review windows open, and where each has got to.
            "reviewWindows": crate::review_window::dump(cx),
            // The page in the main panel in place of a thread (`showsHomePage`).
            "homePage": match self.home_page() {
                Some(SidebarMode::Scheduled) => self.scheduled_home_dump(),
                Some(SidebarMode::Projects) => json!({ "title": "Projects" }),
                Some(_) => self.librarian_home_dump(),
                None => Value::Null,
            },
            // The librarian's state in words, once the runner has said.
            "librarian": librarian_state,
            // The transcript on show: a sub-agent's in place of its thread's.
            "transcript": self.subagent_pane.as_ref().map(|p| p.read(cx)).or(pane).map(|p| p.dump(cx)).unwrap_or_else(|| json!({ "rows": [] })),
            "subagent": self.subagent.as_ref().map(|(_, id)| id.clone()),
            "decisions": pane.and_then(|p| p.composer.read(cx).decision_lines(cx)),
            // A project's page in place of the thread, and the Projects page's
            // cards while it's showing (projects.rs, projects_page.rs).
            "project": self.project_page_dump(),
            "projectCards": self.project_cards_dump(),
            "projectDecisions": self.project_decisions_dump(),
            "banner": self.banner_dump(),
            "limit": self.limit_dump(cx),
            "error": self.error_message.clone(),
            "dock": dock,
            // Folded away with ⌘/ or closed, it isn't on screen (thread_panels.rs).
            "threadsShowing": self.threads_showing(cx),
            // Threads in panels of their own, and the one the main panel says
            // is open in its own panel (`OpenElsewhereNote`).
            "threadPanels": self.thread_panels_dump(cx),
            "openElsewhere": self.open_elsewhere(cx),
            // The keys shown beside the buttons while ⌘ is held (key_hints.rs).
            "keyHints": self.key_hints_dump(cx),
            "zoom": crate::style::zoom_percent(),
            "archivedNote": self.archived_note.as_ref().map(|n| n.thread.clone()),
            "inboxEndNote": self.inbox_end_note.as_ref().map(|n| if n.empty { "empty" } else { "end" }),
            "rails": self.rails_dump(cx),
            // What the activity bar's Runners, Updates and Connectors panels say.
            "runners": self.runners_dump(cx),
            "updates": self.updates_dump(cx),
            "connectors": self.connectors_dump(cx),
            "usage": self.usage_dump(cx),
            "header": self.header_dump(),
            "panels": self.side.read(cx).dump(cx),
            // Where the web runtime has got to (web_runtime.rs).
            "webRuntime": crate::web_runtime::dump(cx),
            // Diagrams and pages open full size, by block id (rich_window.rs).
            "richWindows": crate::rich_boxes::windows_dump(cx),
            // The web panels' pages, and the links that left them (web_panels.rs).
            "webPanels": crate::web_panels::dump(cx),
            "actions": self.side.read(cx).thread(cx).map(|t| crate::patches::actions_dump(self.side.read(cx), &t)).unwrap_or_else(|| json!([])),
            "terminal": crate::terminal::dump(self, cx),
        });
        // Any other note the window shows outside the banner and the error
        // bar: the sidebar's, when the list couldn't be read. Never the
        // connection's (Mike, 12 Nov: the raw error mid-restart).
        dump["notes"] = json!(self.list_error.iter().collect::<Vec<_>>());
        // What came across from the Swift app, while its note's up. Set
        // after, as the literal above is as long as serde_json's macro goes.
        dump["importNote"] = json!(self.import_note);
        // Videos and PDFs up in the system's views, in any window (media_view.rs).
        dump["media"] = crate::media_view::dump();
        // The card a page swipe shows while it's under way (swipe_nav.rs).
        dump["pageSwipe"] = self.page_swipe_dump();
        // The last archive or hibernate flight: where it started and aimed,
        // and once it's landed, where the heading's box was (archive_flight.rs).
        dump["flight"] = self.flight.dump();
        // Whether motion is reduced (automation.rs): no flights or glides then.
        dump["reduceMotion"] = json!(cx.reduce_motion());
        // The last time the panels moved (a panel opened or closed): where
        // each panel was and went, and whether they're still on their way.
        dump["panelMotion"] = self.dock_view.read(cx).last_motion().map_or(Value::Null, |(from, to, moving)| {
            let rects = |frames: &std::collections::HashMap<String, threadbear_dock::Rect>| -> Value {
                frames.iter().map(|(panel, r)| (panel.replace("threadbear.", ""), json!({ "x": r.x.round(), "y": r.y.round(), "width": r.width.round(), "height": r.height.round() }))).collect::<serde_json::Map<_, _>>().into()
            };
            json!({ "from": rects(from), "to": rects(to), "moving": moving })
        });
        dump
    }

    /// Whether the panels are gliding or an archive flight is under way
    /// (automation's `motion`).
    #[cfg(feature = "automation")]
    pub(crate) fn moving(&self, cx: &App) -> bool {
        self.dock_view.read(cx).last_motion().is_some_and(|(_, _, moving)| moving) || self.flight.in_flight()
    }

    /// The composer's part of the dump (AppDump.composer).
    #[cfg(feature = "automation")]
    fn dump_composer(&self, cx: &App) -> Value {
        let composer = self.composer(cx);
        let composer = composer.as_ref().map(|c| c.read(cx));
        let mut out = json!({
            "draft": self.selected.clone().unwrap_or_else(|| "new".into()),
            "text": composer.map(|c| c.draft_text(cx)).unwrap_or_default(),
            "images": composer.map_or(0, |c| c.draft_images(cx)),
            "creating": self.selected.is_none() && self.new_thread.creating,
        });
        if let Some(composer) = composer {
            let data = composer.chip_data(cx);
            if let Some(t) = &data.summary {
                out["cwd"] = json!(t.cwd);
                out["account"] = json!(t.account_id);
                out["mode"] = json!(if t.permission_mode.is_empty() { "full" } else { t.permission_mode.as_str() });
            }
            if let Some(away) = &data.away {
                out["runner"] = json!(away.name);
            }
            out["chip"] = json!(crate::composer_chips::model_chip_text(&data));
            // The seq of the message being edited, as the Swift dump has it.
            out["editing"] = json!(composer.editing.as_ref().map(|e| e.seq));
            out["chipBadge"] = json!(crate::composer_chips::model_chip_badge_text(&data));
            // The box and its card locked while the thread moves, with what
            // the box says in place of its words.
            out["moving"] = json!(composer.moving_line(cx));
            // The refusal offer the box has over it (RefusalOffer), as its
            // button reads.
            let refused = composer.thread_id.is_some() && self.open_thread().and_then(|t| self.refusal_target(t, cx)).is_some();
            out["offerAbove"] = json!(refused.then_some(if self.continuing_refusal.is_some() { "Continuing…" } else { "Continue with Codex Sol" }));
            // Whether the chips and the send button fit inside the box.
            let (row, send) = composer.controls_fit.get();
            out["chipsFit"] = json!(send <= row + 0.5);
            // The chips spinning while a login, pool or folder change lands.
            out["busy"] = json!(composer.busy_chips(cx));
            if let Some(card) = composer.card_lines(cx) {
                out["card"] = json!(card);
            }
            // The picks shown as chips under a folded card (composer_answers.rs).
            if let Some(answers) = composer.answer_chip_lines(cx) {
                out["answers"] = json!(answers);
            }
            if let Some(runs) = crate::card_run::composer_lines(composer, cx) {
                out["cardRuns"] = json!(runs);
            }
        }
        out
    }

    /// The sections with the search applied, as they're drawn.
    pub(crate) fn shown_sections(&self, cx: &App, now: i64) -> Vec<inbox::Section> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let wakes = inbox::wakes(&self.scheduled.list);
        inbox::sections_ordered(self.threads.values().filter(|t| query.is_empty() || inbox::matches(t, &query)), now, self.sidebar.order, &wakes)
    }

    /// The connection as the Swift app tells it (`ConnectionState`). While it
    /// tries again after a drop it stays what the drop was, as the Swift
    /// app's does, rather than "Connecting" on each try.
    pub(crate) fn connection(&self) -> Connection<'_> {
        use threadbear_client::runner::Disconnect;
        let drop = match &self.status {
            Status::Connected { .. } => return Connection::Connected,
            Status::Disconnected { kind, reason, .. } => Some((*kind, reason.as_str())),
            Status::Connecting { .. } => self.last_drop.as_ref().map(|(kind, reason)| (*kind, reason.as_str())),
        };
        match drop {
            None => Connection::Connecting,
            Some((Disconnect::Down, reason)) => Connection::Down(reason),
            Some((Disconnect::Lost, reason)) => Connection::Lost(reason),
            Some((Disconnect::Refused, reason)) => Connection::Unauthorized(reason),
        }
    }

    /// "Start runner" kickstarts the installed runner's LaunchAgent, which
    /// only helps an app pointed at that runner's data folder: an app on the
    /// mock or a dev runner must never start the live one (`canStartRunner`).
    pub(crate) fn can_start_runner(&self) -> bool {
        cfg!(target_os = "macos") && {
            let home = threadbear_client::config::RunnerConfig::from_env().home;
            let default = threadbear_client::config::default_home();
            home == default || home.canonicalize().ok().zip(default.canonicalize().ok()).is_some_and(|(a, b)| a == b)
        }
    }

    /// Start runner: launchctl, then a fresh try a moment later, as the
    /// service takes a little while to listen (`startRunner`).
    pub(crate) fn start_runner(&mut self, cx: &mut Context<Self>) {
        if self.starting_runner || !self.can_start_runner() {
            return;
        }
        self.starting_runner = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let failure = cx.background_executor().spawn(async { crate::runner_launcher::start() }).await;
            cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
            this.update(cx, |this, cx| {
                this.starting_runner = false;
                if let Some(failure) = failure {
                    this.error_message = Some(failure);
                }
                this.reconnect_now(cx);
            })
            .ok();
        })
        .detach();
    }

}

/// `Threadbear::connection`: the Swift app's `ConnectionState`, each drop
/// with what went wrong.
pub(crate) enum Connection<'a> {
    Connected,
    Connecting,
    Down(&'a str),
    Lost(&'a str),
    Unauthorized(&'a str),
}

/// `change`'s fields written over `into`'s, objects within objects too, as a
/// `settings.update` changes only what it names.
fn merge_json(into: &mut Value, change: &Value) {
    match (into, change) {
        (Value::Object(into), Value::Object(change)) => {
            for (key, value) in change {
                match into.get_mut(key) {
                    Some(existing) if existing.is_object() && value.is_object() => merge_json(existing, value),
                    _ => {
                        into.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (into, change) => *into = change.clone(),
    }
}

pub(crate) fn project_list(projects: &Value) -> Vec<(String, String, bool)> {
    projects.as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|p| Some((p["id"].as_str()?.to_string(), p["title"].as_str()?.to_string(), p["archived"].as_bool().unwrap_or(false)))).collect())
}


impl Render for Threadbear {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.menus_stale) {
            self.sync_menus(cx);
        }
        let open_menu = self.render_open_menu();
        #[cfg(feature = "frame-stats")]
        let open_menu = open_menu.into_iter().map(IntoElement::into_any_element).chain(self.frame_rate.clone().map(|f| f.into_any_element()));
        // The web panels' pages are native views: shown and placed once the
        // window's laid out, where their panels were drawn (web_panels.rs).
        let pages = crate::web_panels::sweeper(self.open_menu_at(), self.share_popover.is_some());
        // Videos and PDFs in the system's views the same way (media_view.rs),
        // hidden under the picture viewer and the pane's own menus too.
        let media = {
            let side = self.side.read(cx);
            let everything = self.share_popover.is_some() || side.viewer.is_open();
            let menus = self.open_menu_at().into_iter().chain(side.menu.as_ref().map(|m| m.position)).collect();
            crate::media_view::sweeper(window, crate::media_view::Cover { everything, menus })
        };
        // Last: every press in the window, for the activity log (tips.rs).
        self.render_shell(window, cx).children(open_menu).child(pages).child(media).child(crate::tips::window_presses())
    }
}
