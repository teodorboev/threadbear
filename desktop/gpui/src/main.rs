// Threadbear as a native GPUI app: the runner connection (../client), drawn
// with GPUI and gpui-kit's components. GPUI has no tokio, so the
// connection runs on a runtime of its own and hands its events across on a
// channel; requests run there too and their results come back as tasks.
// Read-outs that exist only for the scripted control (`automation.rs`).
#![cfg_attr(not(feature = "automation"), allow(dead_code, unused_imports))]
mod a11y;
mod activity;
mod activity_panels;
mod agent_browser;
mod about_window;
mod alert;
mod app;
mod app_log;
mod archive_flight;
mod app_update;
#[cfg(feature = "automation")]
mod automation;
#[cfg(feature = "automation")]
mod automation_compat;
#[cfg(any(test, feature = "automation"))]
mod automation_screenshot;
mod chips;
mod colour_space;
mod composer_media;
#[cfg(target_os = "macos")]
mod composer_url;
mod composer;
mod composer_card;
mod composer_account;
mod composer_answers;
mod composer_chips;
mod composer_context;
mod composer_inline;
mod composer_send;
mod composer_spelling;
mod connectors_panel;
mod crash_reporting;
mod deep_links;
mod ding;
mod draft_boxes;
mod drafts;
mod explain_pane;
mod far_rows;
mod file_drag;
mod file_edit;
mod file_find;
mod file_watch;
mod files;
mod files_store;
mod folders;
#[cfg(feature = "frame-stats")]
mod frame_stats;
mod hang_watch;
mod handover;
mod input_probe;
mod header;
mod highlights;
mod highlights_board;
mod highlights_project;
mod highlights_store;
mod home_page;
mod icons;
mod installed;
mod key_hints;
mod librarian;
mod mac_switch;
mod media_view;
mod move_sheet;
mod new_thread;
mod notify;
mod open_anything;
mod open_gate;
mod open_motion;
mod pane_cache;
mod pane_store;
mod patch_icons;
mod patches;
mod pinned_panels;
mod prefs;
mod project_decisions;
mod project_page;
mod projects;
mod projects_list;
mod projects_page;
mod push_button;
mod quit_prompt;
mod rails;
mod remote;
mod remote_pictures;
mod reply_buttons;
mod reply_links;
mod review_window;
mod rich_boxes;
mod rich_window;
mod row_motion;
mod row_swipe;
mod rows;
mod scripted;
mod runner_launcher;
mod runners_panel;
mod schedule_editor;
mod scheduled;
mod selection_bar;
mod settings;
mod settings_window;
mod side_sections;
mod shell;
mod shell_blocks;
mod sidebar;
mod slash_palette;
mod slash_resize;
mod spinner;
mod style;
mod swift_import;
mod milestones;
mod swipe_nav;
mod terminal;
mod terminal_element;
mod thread;
mod text_services;
mod native_text;
mod thread_map;
mod tips;
mod thread_panels;
mod tour;
mod setup_panel;
mod unsent;
mod updates_panel;
mod usage;
mod viewer;
mod web_panels;
mod web_runtime;
mod window_menu;

use app::Threadbear;
use gpui_kit::*;
use serde_json::Value;
use settings::Settings;
use std::sync::{Arc, LazyLock};
use threadbear_client::runner::{Client, RpcError, Sink, Status};
use threadbear_core::zoom;
use tokio::sync::mpsc;

pub static RT: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("a tokio runtime"));

pub enum Seen {
    Status(Status),
    Note(String, Value),
}

pub(crate) struct ChannelSink(pub(crate) mpsc::UnboundedSender<Seen>);

impl Sink for ChannelSink {
    fn status(&self, status: &Status) {
        let _ = self.0.send(Seen::Status(status.clone()));
    }
    fn notification(&self, method: &str, params: &Value) {
        let _ = self.0.send(Seen::Note(method.to_string(), params.clone()));
    }
}

/// A request on the connection's runtime, awaited from GPUI's.
pub async fn call(client: Arc<Client>, method: impl Into<String>, params: Value) -> Result<Value, RpcError> {
    let method = method.into();
    // Local Windows files are opened under the desktop's path guard, once.
    // A runner resolving the same raw path again can follow a different
    // link, and a directory listing can stat a child link into an SMB share.
    #[cfg(windows)]
    if params["runnerId"].is_null() && matches!(method.as_str(), "files.read" | "files.list") {
        return RT.spawn_blocking(move || {
            let result = if method == "files.read" { threadbear_core::local_files::read(&params) } else { threadbear_core::local_files::list(&params) };
            result.map_err(|e| RpcError { code: -32602, message: e.to_string(), data: None })
        }).await.unwrap_or_else(|e| Err(RpcError { code: -1, message: e.to_string(), data: None }));
    }
    RT.spawn(async move { client.call(&method, params).await })
        .await
        .unwrap_or_else(|e| Err(RpcError { code: -1, message: e.to_string(), data: None }))
}

/// `threads.update` (a rename, a pick, an archive) for a thread: one of this
/// runner's goes in restart order while it's connected (`inRestartOrder`),
/// so a pick the restarting runner turns away goes again on the new one,
/// and nothing made later lands before it. With no connection it waits for
/// a restarting runner, or fails if none is restarting; another Mac's
/// thread goes as it is.
/// Its place in that order is taken as this is called, where the click is
/// handled, so call it there and await it later, not inside a task.
pub fn call_for_thread(client: Arc<Client>, remote: bool, method: &'static str, params: Value) -> impl std::future::Future<Output = Result<Value, RpcError>> + 'static {
    // Connected or not: while a restart's under way it waits for the new
    // runner in its place, and with none it fails (Codex's review, 12 Nov:
    // one made while down skipped the queue once the new runner was up).
    let place = (!remote).then(|| client.place(params["threadId"].as_str().unwrap_or("")));
    async move {
        let Some(place) = place else {
            return call(client, method, params).await;
        };
        RT.spawn(async move { client.call_in_restart_order_at(place, method, params, Arc::new(|_| {})).await })
            .await
            .unwrap_or_else(|e| Err(RpcError { code: -1, message: e.to_string(), data: None }))
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Where the window opens: where it was left, if that's still on a screen
/// (`MainWindowFrame.track`), else centred at the Swift app's default size.
fn initial_bounds(settings: &Settings, cx: &App) -> Bounds<Pixels> {
    let screens: Vec<zoom::Rect> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            zoom::Rect { x: f64::from(b.origin.x), y: f64::from(b.origin.y), width: f64::from(b.size.width), height: f64::from(b.size.height) }
        })
        .collect();
    let saved = settings.saved.window_frame.filter(|f| zoom::on_screen(zoom::Rect { x: f.x, y: f.y, width: f.width, height: f.height }, &screens));
    match saved {
        Some(f) => Bounds { origin: point(px(f.x as f32), px(f.y as f32)), size: size(px(f.width as f32), px(f.height as f32)) },
        None => Bounds::centered(None, size(px(1180.), px(800.)), cx),
    }
}

fn main() {
    // A scripted app against the real data folder stops here, and one that
    // goes on gets a pasteboard of its own, before the platform reads it
    // (scripted.rs).
    scripted::guard();
    // A build staged while the app last ran, put in place and the new app
    // opened once this one's gone: before anything else (app_update.rs).
    // Not the new copy of a handover: the old one put it in place already,
    // and is still running from the bundle a swap would clear away.
    if !handover::window_options_hidden() && app_update::apply_staged_at_launch() {
        std::process::exit(0);
    }
    // The new copy of a handover says it's started (handover.rs).
    handover::announce_start();
    // What it was built from, for the runner to tell a thread waiting for
    // the app to restart (docs/protocol.md, "App restarts").
    app_update::announce_build();
    // Before anything else, to catch a panic at launch. Only the installed
    // release, and a scripted app into its own folder (crash_reporting.rs).
    crash_reporting::start();
    // The app's own log (app_log.rs), before the connection's first line.
    app_log::start();
    // All of Lucide (about 1 MB), so `icons.rs` can name any icon; the default
    // `Assets` has only gpui-kit's own hundred.
    let application = scripted::application(gpui_kit::application().with_assets(gpui_kit::assets::AllAssets));
    // threadbear://thread/<id> links from outside (deep_links.rs), queued from
    // now: macOS can hand one over before the window's made.
    let links = deep_links::receive(&application);
    application.run(|cx| {
        gpui_kit::init(cx);
        // An untouched caret should not redraw the window indefinitely.
        // It stays visible after ten seconds and resumes on editing or focus.
        gpui_kit::component::Theme::update(cx, |theme| {
            theme.cursor_blink = theme.cursor_blink.with_idle_timeout(std::time::Duration::from_secs(10));
        });
        native_text::init(cx);
        // Every window's colours as sRGB, before the first opens (colour_space.rs).
        colour_space::install(cx);
        composer_media::install(cx);
        // The main thread watched for freezes from here on (hang_watch.rs).
        hang_watch::start(cx);
        // Whether the web runtime is installed; nothing starts until a page needs it.
        web_runtime::init(cx);
        // The diagrams and pages in replies, drawn through it.
        rich_boxes::init(cx);
        // And the panels' pages, the Convex dashboard's among them.
        web_panels::init(cx);
        // Videos and PDFs in the system's views: their calls back (a PDF's
        // links, to the open gate) and the menus they hide under.
        media_view::init(cx);
        let mut settings = Settings::load();
        // The Swift app's settings and drafts, brought across once, before
        // anything reads them (swift_import.rs).
        let brought = swift_import::run_once(&mut settings, cx);
        // And whether they'd seen its tour, apart (swift_import.rs).
        swift_import::tour_once(&mut settings);
        let bounds = initial_bounds(&settings, cx);
        // Back from an update's restart made while another app was in front:
        // the window comes back behind it, not taking the keys (app_update.rs).
        // A dev-app.sh instance never takes the keys (scripted.rs).
        // The new copy of a handover opens unseen, under the old window
        // (handover.rs), and takes the keys only at "go".
        let hidden = handover::window_options_hidden();
        let focus = !hidden && !scripted::keep_behind() && settings.saved.update_restore.as_ref().is_none_or(|r| r["front"].as_bool() != Some(false));
        let (window, app) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // No title bar: the panels' bars are the top of the window,
                // with the traffic lights over the top-left panel's bar and
                // the bars moving the window (ThreadbearApp, BearDock's
                // README says what a hosting window must set).
                titlebar: Some(TitlebarOptions { title: Some("Threadbear".into()), appears_transparent: true, traffic_light_position: Some(point(px(9.), px(10.))) }),
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(760.), px(480.))),
                app_id: Some("threadbear".into()),
                focus,
                show: !hidden,
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| Threadbear::new(settings, window, cx)),
        )
        .expect("failed to open the window");
        // Its close quits, and File's and Window's commands reach whichever
        // window is in front (window_menu.rs).
        window.update(cx, |_, w, cx| window_menu::install(w, cx)).ok();
        if scripted::keep_behind() {
            window.update(cx, |_, w, _| scripted::send_behind(w)).ok();
        }
        if hidden {
            window.update(cx, |_, w, _| handover::conceal(w)).ok();
        }
        deep_links::listen(links, app.downgrade(), window, cx);
        if let Some(note) = brought {
            app.update(cx, |t, cx| t.show_import_note(note, cx));
        }
    });
}
