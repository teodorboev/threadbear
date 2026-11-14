// Scripted control of a real window, for the behaviour specs (features/) and
// agents checking their own work. Only a build with the `automation` feature
// has this: a release never does. It takes the same JSON commands as the Swift
// app's automation (app/README.md, "Seeing and driving it from a script"), and
// its `dump` has the same shape, so one set of specs drives both.
//
// Commands arrive over a local socket, each line one JSON object carrying the
// token from the file named by THREADBEAR_AUTOMATION, and run on the app's own
// thread between frames. Clicks and keys are real GPUI events going through
// real hit testing (gpui-kit's test helpers work on a live window too), so a
// spec presses what a person would press.
use crate::app::Threadbear;
use crate::{call, RT};
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::*;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

type Request = (Value, oneshot::Sender<Value>);

/// Starts listening if THREADBEAR_AUTOMATION names a file to say where.
pub fn serve(app: WeakEntity<Threadbear>, window: AnyWindowHandle, cx: &mut App) {
    let Ok(file) = std::env::var("THREADBEAR_AUTOMATION") else { return };
    // Reduce Motion as the script asks ("on" or "off"), not as the machine
    // has it: gpui-base reads the system's switch at start, and GitHub's
    // macos-26 runners have it on, so the panels never glided and the archive
    // flight only faded there while every other Mac played them (12 Nov).
    // Set after gpui-base's reading, it's the app's from then on, and a later
    // reading leaves it alone (gpui-base's reduce_motion.rs).
    match std::env::var("THREADBEAR_REDUCE_MOTION").as_deref() {
        Ok("on") => cx.set_reduce_motion(true),
        Ok("off") => cx.set_reduce_motion(false),
        _ => {}
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Request>();
    RT.spawn(listen(file, tx));
    cx.spawn(async move |cx| {
        while let Some((cmd, reply)) = rx.recv().await {
            // What was run, by name (never its words: a `send`'s text is
            // Mike's), as the Swift app's AppLog has them. Not the dumps, which
            // a spec asks for many times a second.
            let name = cmd["cmd"].as_str().unwrap_or("").to_owned();
            let method = cmd["method"].as_str().map(|m| format!(" {m}")).unwrap_or_default();
            if name != "dump" {
                threadbear_client::log::write("automation", &format!("{name}{method}"));
            }
            let out = run(&app, window, &cmd, cx).await;
            if let Err(e) = &out {
                threadbear_client::log::write("automation", &format!("{name}{method} failed: {e}"));
            }
            let line = match out {
                Ok(result) => json!({ "result": result }),
                Err(error) => json!({ "error": error }),
            };
            let _ = reply.send(line);
        }
    })
    .detach();
}

async fn listen(file: String, tx: mpsc::UnboundedSender<Request>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("a local port");
    let port = listener.local_addr().expect("its port").port();
    let token = threadbear_client::auth::new_nonce();
    write_private(&file, &json!({ "port": port, "token": token }).to_string());
    loop {
        let Ok((socket, _)) = listener.accept().await else { continue };
        let (tx, token) = (tx.clone(), token.clone());
        RT.spawn(async move {
            let (read, mut write) = socket.into_split();
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let cmd: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                let reply = if cmd["token"].as_str() == Some(token.as_str()) {
                    let (back, answer) = oneshot::channel();
                    let name = cmd["cmd"].as_str().unwrap_or("").to_owned();
                    if tx.send((cmd, back)).is_err() {
                        // The app's command loop is gone for good: say so
                        // rather than close the connection with nothing.
                        let _ = write.write_all(format!("{}\n", unanswered(&name, false)).as_bytes()).await;
                        break;
                    }
                    answer.await.unwrap_or_else(|_| unanswered(&name, true))
                } else {
                    json!({ "error": "wrong token" })
                };
                if write.write_all(format!("{reply}\n").as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}

/// A command the app took and never answered. All the socket's side knows
/// is that the app's thread let go of it: a panic there, or the app quitting,
/// with the process perhaps still running. It used to say "the app stopped",
/// which read as the process ending and hid the panic a spec on the cloud box
/// needed (13 Nov). `unanswered` tells the spec driver (features/support/gpui.ts)
/// to gather the process's state, kept panics and log, and the line goes in
/// the app's log too, as its name only (never its words).
fn unanswered(name: &str, taken: bool) -> Value {
    let error = if taken {
        format!("the app dropped `{name}` without answering: its command loop let go of it (a panic on the app's thread, or the app quitting); the process may still be running")
    } else {
        format!("the app's command loop has ended, so `{name}` never ran; the process may still be running")
    };
    threadbear_client::log::write("automation", &format!("{name} unanswered: {}", if taken { "dropped" } else { "loop ended" }));
    json!({ "error": error, "unanswered": if taken { "dropped" } else { "loop ended" } })
}

// The port and token, written whole and readable only by this user: a spec
// waits for the file to appear, so it must never see half of one.
fn write_private(file: &str, text: &str) {
    threadbear_core::private_file::write(std::path::Path::new(file), text.as_bytes()).expect("the automation file");
}

async fn run(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, cmd: &Value, cx: &mut AsyncApp) -> Result<Value, String> {
    if cmd["cmd"] == "mock" {
        let method = format!("mock.{}", cmd["method"].as_str().unwrap_or(""));
        let client = app.read_with(cx, |t, _| t.client.clone()).map_err(|e| e.to_string())?;
        return call(client, method, cmd["params"].clone()).await.map_err(|e| e.message);
    }
    // What a restart onto a staged build by itself would wait for now
    // (app_update.rs), read as its once-a-second look reads it: the windows
    // first, outside any update, then the app. A scripted app never restarts.
    if cmd["cmd"] == "updates" && cmd["restartCheck"].as_bool() == Some(true) {
        let main = app.read_with(cx, |t, _| t.main_window).map_err(|e| e.to_string())?;
        let windows = cx.update(|cx| crate::app_update::windows_now(main, cx));
        let (m, line) = app.read_with(cx, |t, cx| (t.restart_moment(windows, cx), threadbear_core::updates::app_line(&t.app_update_side()))).map_err(|e| e.to_string())?;
        return Ok(json!({
            "staged": m.staged, "loaded": m.loaded, "hasDrafts": m.has_drafts, "held": m.held,
            "otherWindowInput": m.other_window_input, "active": m.active, "idleFor": m.idle_for, "line": line,
        }));
    }
    // The runners' figures, asked for and waited on, as the Swift command does.
    if cmd["cmd"] == "runners" && cmd["stats"].as_bool() == Some(true) {
        return crate::runners_panel::stats_command(app, cx).await;
    }
    // `send` from the new-thread box waits for the thread it starts and says
    // its id, as the Swift command does, for `tb show` and the like.
    let from_new = cmd["cmd"] == "send" && app.read_with(cx, |t, _| t.selected.is_none()).map_err(|e| e.to_string())?;
    // A sub-agent's transcript, as a click on its row does, waited on until
    // it's read, as the Swift command does: {"thread": id or its start, "id"}.
    if cmd["cmd"] == "subagent" {
        let sub = cmd["id"].as_str().filter(|s| !s.is_empty()).ok_or("id is the sub-agent's id")?.to_owned();
        let thread = app
            .read_with(cx, |t, _| match cmd["thread"].as_str() {
                Some(start) => t.thread_id_from(start),
                None => t.selected.clone().ok_or_else(|| "no thread selected".to_owned()),
            })
            .map_err(|e| e.to_string())??;
        let opener = app.clone();
        cx.update_window(window, |_, window, cx| {
            crate::thread::scripted_input(|| {
                opener.update(cx, |t, cx| {
                    t.set_mode(crate::shell::SidebarMode::Inbox, cx);
                    t.open_subagent(thread.clone(), sub.clone(), window, cx);
                })
            })
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        let started = std::time::Instant::now();
        let wait = cmd["timeout"].as_f64().unwrap_or(10.);
        loop {
            let state = app.read_with(cx, |t, cx| t.subagent_pane.as_ref().map(|p| p.read(cx).read_state())).map_err(|e| e.to_string())?;
            match state {
                Some((true, _, _, rows)) => return Ok(json!(format!("ok {sub} {rows} rows"))),
                Some((_, true, _, _)) => return Ok(json!(format!("ok {sub} no record"))),
                Some((_, _, Some(failed), _)) => return Err(format!("couldn't load: {failed}")),
                _ => {}
            }
            if started.elapsed().as_secs_f64() > wait {
                return Err(format!("timed out loading {sub}"));
            }
            cx.background_executor().timer(std::time::Duration::from_millis(20)).await;
        }
    }
    // The web runtime (web_runtime.rs): its install, Not now, and a check
    // that it draws, each waited on as asked.
    if cmd["cmd"] == "native-text" { return crate::native_text::automation(window, cmd, cx).await; }
    if cmd["cmd"] == "webRuntime" {
        return crate::web_runtime::automation(cmd, cx).await;
    }
    // A web panel's page (web_panels.rs): run a script in it, click in it as
    // a person would, or wait for it to load.
    if cmd["cmd"] == "webPanel" {
        return crate::web_panels::automation(cmd, cx).await;
    }
    if cmd["cmd"] == "convex" {
        return crate::web_panels::convex_command(app, cmd, cx).await;
    }
    // `text` typed as a person's keys are, through AppKit, to whoever has the
    // window's keyboard (`keyboard` in the dump): a page's view takes them
    // from GPUI's. Out here, not in an update, as AppKit hands them straight
    // to GPUI, which updates the window itself.
    if cmd["cmd"] == "native-keys" {
        // A frame first, as a person's keys come after the screen has caught
        // up with their last one: a scripted window behind others draws only
        // when asked, and it's a frame that places the pages.
        cx.update_window(window, |_, window, cx| window.render_frame(cx)).map_err(|e| e.to_string())?;
        return crate::web_panels::send_keys(window, text(cmd, "text"), cx).map(|()| json!("ok"));
    }
    // The activity log (activity.rs), as the Swift app's `activity`: a press
    // or a key, then what today's file has, a moment later.
    if cmd["cmd"] == "activity" {
        return crate::activity::automation(app, window, cmd, cx).await;
    }
    let weak = app.clone();
    // Two fingers across a sidebar row or the window, and a row dragged with
    // the mouse (row_swipe.rs, swipe_nav.rs): real events through the
    // window's dispatch, a sixtieth of a second apart, as the Swift commands.
    if cmd["cmd"] == "swipe" {
        return crate::row_swipe::swipe_command(&weak, window, cmd, cx).await;
    }
    if cmd["cmd"] == "row-drag" {
        return crate::row_swipe::row_drag_command(&weak, window, cmd, cx).await;
    }
    if cmd["cmd"] == "page-swipe" {
        return crate::swipe_nav::page_swipe_command(&weak, window, cmd, cx).await;
    }
    // A picture or file dragged with the mouse (file_drag.rs).
    if cmd["cmd"] == "file-drag" {
        return crate::file_drag::file_drag_command(&weak, window, cmd, cx).await;
    }
    // A design review's window (review_window.rs), as the Swift command.
    if cmd["cmd"] == "review" {
        return crate::review_window::automation(&weak, window, cmd, cx).await;
    }
    // Send's right-click menu (composer_send.rs), read the same way.
    // The slash palette's right-click menu (slash_resize.rs), the same way.
    if cmd["cmd"] == "palette-menu" {
        return crate::slash_resize::menu_command(window, cmd, cx).await;
    }
    if cmd["cmd"] == "composer-menu" && cmd["menu"] == "send" {
        return crate::composer_send::menu_command(window, cmd, cx).await;
    }
    // A reply link's right-click menu (reply_links.rs): the kit builds it
    // after the press, so it's read in a turn of its own.
    if cmd["cmd"] == "peek" && ["menu", "click", "default"].iter().any(|k| cmd.get(*k).is_some()) {
        return crate::reply_links::menu_command(&weak, window, cmd, cx).await;
    }
    let Some(app) = app.upgrade() else { return Err("the app has gone".into()) };
    // `"settings": true`: in the Settings window, as the Swift app's `click`
    // and `shot --settings` reach it; `"review": id`, in that review's.
    let window = if let Some(id) = cmd["review"].as_str() {
        cx.update(|cx| crate::review_window::window_for(id, cx)).ok_or("that review isn't open")?
    } else if cmd["den"].as_bool() == Some(true) && cmd["cmd"] != "den" {
        // `"den": true`: in the About window (a `screenshot` of it).
        cx.update(crate::about_window::handle).ok_or("the About window isn't open")?
    } else if cmd["settings"].as_bool() == Some(true) && cmd["cmd"] != "settings" {
        app.read_with(cx, |t, _| t.settings_window.as_ref().map(|(handle, _)| *handle)).ok_or("the Settings window isn't open")?
    } else if let Some(panel) = cmd["panel"].as_str().filter(|_| matches!(cmd["cmd"].as_str(), Some("window" | "point" | "size" | "resize" | "screenshot" | "click-at"))) {
        app.read_with(cx, |t, cx| t.dock_view.read(cx).floating_window(panel)).ok_or("that panel has no window of its own")?
    } else {
        window
    };
    if cmd["cmd"] == "screenshot" {
        let path = std::path::PathBuf::from(text(cmd, "path"));
        let image = cx.update_window(window, |_, window, cx| {
            // A covered window gets no system frames. Draw its latest state
            // before reading the pixels, including pictures just loaded.
            window.render_frame(cx);
            window.render_to_image().map_err(|e| e.to_string())
        }).map_err(|e| e.to_string())??;
        // Encoding and disk writes must not stall the app's thread. Await
        // the save so the socket replies only when this shot is complete.
        cx.background_executor().spawn(async move { crate::automation_screenshot::save(&image, &path) }).await?;
        return Ok(json!("ok"));
    }
    // The first-run tour (tour.rs): {"do": "start"|"next"|"back"|"end"|"pools"},
    // or nothing to read where it's at. Says the step, its title and what it
    // lights up, once the card and the spotlight have had their move.
    if cmd["cmd"] == "tour" {
        cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| app.update(cx, |t, cx| t.tour_command(cmd, window, cx)))).map_err(|e| e.to_string())??;
        cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
        // Drawn twice: the marks are measured in one frame, the overlay
        // placed by them in the next.
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .ok();
        return Ok(json!(cx.update(|cx| app.read(cx).tour_line(cx))));
    }
    // Until what the window shows holds every condition given (or with
    // "not", none), as the Swift command: read from the same `dump` a
    // script reads next (threadbear_core::wait_for), every 50 ms.
    if cmd["cmd"] == "waitFor" {
        let wait = threadbear_core::wait_for::WaitFor::from_command(cmd)?;
        let started = std::time::Instant::now();
        let dump = json!({ "cmd": "dump" });
        loop {
            let shown = cx.update_window(window, |_, window, cx| step(&app, &dump, window, cx)).map_err(|e| e.to_string())??;
            if wait.holds(&shown) {
                return Ok(json!(format!("ok {} ms", started.elapsed().as_millis())));
            }
            if started.elapsed().as_secs_f64() >= wait.timeout_seconds {
                return Err(format!("timed out after {} s waiting for {}", wait.timeout_seconds, wait.describe()));
            }
            cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
        }
    }
    // Its clicks and keys are the script's, not Mike being at the window.
    let out = cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| step(&app, cmd, window, cx))).map_err(|e| e.to_string())??;
    if cmd["cmd"] == "point" && cmd.get("scrollX").is_none() && cmd.get("scrollY").is_none() {
        let hold = cmd["holdMs"].as_f64().filter(|h| h.is_finite() && *h > 0.).unwrap_or(0.).min(30_000.);
        if hold > 0. {
            cx.background_executor().timer(std::time::Duration::from_secs_f64(hold / 1000.)).await;
            cx.update_window(window, |_, window, cx| crate::thread::scripted_input(|| {
                let x = cmd["toX"].as_f64().or(cmd["x"].as_f64()).unwrap_or(0.);
                let y = cmd["toY"].as_f64().or(cmd["y"].as_f64()).unwrap_or(0.);
                window.pointer_up(point(px(x as f32), px(y as f32)), MouseButton::Left, cx);
                window.render_frame(cx);
            })).map_err(|e| e.to_string())?;
        }
    }
    if !from_new {
        return Ok(out);
    }
    let started = std::time::Instant::now();
    let wait = cmd["timeout"].as_f64().unwrap_or(10.);
    loop {
        let (opened, error) = weak.read_with(cx, |t, _| (t.selected.clone(), t.new_thread.error.clone())).map_err(|e| e.to_string())?;
        if let Some(id) = opened {
            return Ok(json!(format!("ok {id}")));
        }
        if let Some(error) = error {
            return Err(format!("not sent: {error}"));
        }
        if started.elapsed().as_secs_f64() > wait {
            return Err(format!("sent, but no new thread showed up within {wait} s"));
        }
        cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
    }
}

/// Where a scripted click lands on the last visible control labelled so
/// (`prefix`: whose label starts so): the middle of what's drawn of it,
/// inside every clip around it, never the middle of its whole bounds.
fn label_point(window: &Window, label: &str, prefix: bool) -> Result<(i32, i32), String> {
    let found = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| if prefix { l.starts_with(label) } else { l == label })).last();
    found.and_then(|e| whole_point(e.visible_bounds())).ok_or_else(|| format!("nothing labelled {label:?} on screen"))
}

/// The middle of `drawn` in whole points, as the specs' tools read them,
/// kept inside it: a sliver 299.6 to 300 wide has its middle at 299.8,
/// which rounds to 300, just past it. `None` when no whole point is in it.
pub(crate) fn whole_point(drawn: Bounds<Pixels>) -> Option<(i32, i32)> {
    let axis = |start: Pixels, end: Pixels| {
        let (start, end) = (f32::from(start), f32::from(end));
        let (low, high) = (start.ceil(), (end - 1.).floor().max(start.ceil()));
        // An end is outside, as GPUI's hit testing has it.
        (low < end).then(|| ((start + end) / 2.).round().clamp(low, high) as i32)
    };
    Some((axis(drawn.left(), drawn.right())?, axis(drawn.top(), drawn.bottom())?))
}

fn text<'a>(cmd: &'a Value, key: &str) -> &'a str {
    cmd[key].as_str().unwrap_or("")
}

/// Everything in the window has settled for this frame, so what a spec reads
/// is what's on screen now.
fn step(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    match text(cmd, "cmd") {
        "point" | "window" | "room" | "account-ids" | "streaming" | "debug-info" | "runners-front" => crate::automation_compat::step(app, cmd, window, cx),
        "size" => shell_step(app, cmd, window, cx),
        // Whether anything's on its way across the window (the panels
        // gliding, an archive flight), which the spec driver asks before
        // every click so it never reads a control's spot mid-move
        // (features/support/gpui.ts, `settle`). Cheap while nothing moves; a
        // frame only while something does, as a covered window gets none
        // from the system and a glide ends at a frame.
        "motion" => {
            if app.read(cx).moving(cx) {
                window.render_frame(cx);
            }
            Ok(json!({ "moving": app.read(cx).moving(cx) }))
        }
        "dump" => {
            // A window covered by another gets no frames from the system on a
            // Mac, and a scroll only takes effect at a frame.
            window.render_frame(cx);
            // The Settings window's too: a section asks for what it shows as
            // it's drawn (Phone & browser's list), as the Swift one on appear,
            // and what's on screen there is read from that frame.
            if let Some((settings, view)) = app.read(cx).settings_window.clone() {
                settings
                    .update(cx, |_, window, cx| {
                        window.render_frame(cx);
                        crate::settings_window::note_drawn(&view, window, cx);
                    })
                    .ok();
            }
            let mut dump = app.read(cx).dump(cx);
            // Who AppKit gives the keys to: GPUI's view, or a page's.
            dump["keyboard"] = crate::web_panels::keyboard_dump(window);
            // Whether the Threads panel has the keyboard (its arrows and Esc).
            if dump["sidebar"].is_object() {
                dump["sidebar"]["keyboard"] = json!(app.read(cx).sidebar.focus.contains_focused(window, cx));
            }
            Ok(dump)
        }
        // The actual menus drawn in this window, including a dock menu
        // that accidentally opened behind a row's. No menu handlers called.
        "context-menus" => {
            window.render_frame(cx);
            let mut menus = Vec::new();
            for index in 0..16 {
                let items: Vec<String> = crate::reply_links::open_menu_items(window, index).into_iter().map(|(title, _)| title).collect();
                if items.is_empty() { break; }
                menus.push(items);
            }
            Ok(json!(format!("ok {}", serde_json::to_string(&menus).unwrap_or_default())))
        }
        "status" => Ok(json!("ok")),
        // Finish setting up (setup_panel.rs), as the Swift command.
        "setup" => Ok(json!(app.update(cx, |t, cx| t.setup_command(cmd, cx)))),
        // Threadbear > About Threadbear, through the menu item, as the Swift
        // command. The window opens just after, so the next ask says what it
        // shows: its lines, " | " between them.
        "den" => {
            if let Some(lines) = crate::about_window::dump(cx) {
                return Ok(json!(format!("ok {}", lines.join(" | "))));
            }
            app.update(cx, |t, cx| t.run_menu_item("About Threadbear", window, cx))?;
            Ok(json!("ok opening"))
        }
        // The Settings window, as the Swift command: opened (on `tab`), or
        // `"close": true` closes it.
        "settings" => {
            if cmd["close"].as_bool() == Some(true) {
                let open = app.update(cx, |t, _| t.settings_window.take());
                if let Some((handle, _)) = open {
                    crate::thread::set_looking(handle.window_id().as_u64(), false);
                    handle.update(cx, |_, window, _| window.remove_window()).ok();
                }
                return Ok(json!("ok closed"));
            }
            let tab = match cmd["tab"].as_str() {
                Some(name) => Some(crate::settings_window::Tab::from_name(name).ok_or_else(|| format!("tab must be one of {}", crate::settings_window::Tab::ALL.map(|t| t.name()).join(", ")))?),
                None => None,
            };
            app.update(cx, |t, cx| t.open_settings(tab, cx));
            // Its sections' buttons and menus, as the Swift command takes them.
            if let Some(done) = crate::settings_window::automation(app, cmd, cx)? {
                return Ok(done);
            }
            Ok(json!("ok"))
        }
        // Settings > Keyboard, as the Swift app's `shortcuts` (settings_window/keyboard.rs).
        "shortcuts" => crate::settings_window::keyboard_automation(app, cmd, cx),
        // The sidebar's commands (sidebar.rs). Guarded ones only take the forms
        // that name a section, a row or a tip; the others fall through below.
        "hover" | "sort" | "archive-more" | "star" | "unstar" | "unarchive" | "realMenu" | "menu-popover" => crate::sidebar::automation(app, cmd, window, cx),
        "new" | "options" => crate::new_thread::automation(app, cmd, window, cx),
        // A threadbear:// link handed over as macOS hands one (deep_links.rs).
        "open-url" => crate::deep_links::automation(app, cmd, window.window_handle(), cx),
        // A thread in a panel of its own, and back (thread_panels.rs).
        "open-panel" | "move-to-main" => crate::thread_panels::automation(app, cmd, window, cx),
        // A panel's right-click menu's own items (Pin, Unpin): what it offers,
        // and with `click`, chosen (pinned_panels.rs).
        "panel-menu" => {
            let panel = cmd["panel"].as_str().ok_or("panel is a panel's id")?.to_owned();
            let offers: Vec<&str> = app.read(cx).pin_menu_label(&panel).into_iter().collect();
            if let Some(click) = cmd["click"].as_str() {
                if !offers.contains(&click) {
                    return Err(format!("{panel}'s menu has no {click}: it offers {offers:?}"));
                }
                app.update(cx, |t, cx| t.choose_pin_menu(&panel, cx));
                window.render_frame(cx);
            }
            Ok(json!(offers))
        }
        // The keys beside the buttons as if ⌘ were held (key_hints.rs).
        "hints" => {
            let on = cmd["on"].as_bool().unwrap_or(true);
            app.update(cx, |t, cx| t.hints_command(on, cx));
            Ok(json!(format!("ok {}", crate::key_hints::showing())))
        }
        // The activity bar's panels, as the Swift app's commands of these names.
        "runners" => crate::runners_panel::automation(app, cmd, window, cx),
        "updates" => crate::updates_panel::automation(app, cmd, window, cx),
        "connectors" => crate::connectors_panel::automation(app, cmd, cx),
        "usage" => crate::usage::automation(app, cmd, cx),
        "move" => crate::move_sheet::automation(app, cmd, window, cx),
        "anything" => crate::open_anything::automation(app, cmd, window, cx),
        "terminal" => crate::terminal::automation(app, cmd, window, cx),
        // As the Swift app's: the window in front and key, for pictures on a
        // desktop of what only draws there (its Terminal's text).
        "front" => {
            cx.activate(true);
            window.activate_window();
            Ok(json!("ok"))
        }
        "expand" | "collapse" if cmd.get("section").is_some() => crate::sidebar::automation(app, cmd, window, cx),
        "expand" | "collapse" if cmd.get("scheduleSection").is_some() => crate::scheduled::automation(app, cmd, cx),
        // Projects (projects.rs): a project's page, its parent, the Projects
        // page's search, and the sidebar's folded project rows.
        "project" | "projectParent" => crate::projects::automation(app, cmd, window, cx),
        "decision" => crate::project_decisions::automation(app, cmd, window, cx),
        "projectTab" => crate::project_decisions::tab_automation(app, cmd, cx),
        "search" if cmd["projects"].as_bool() == Some(true) => crate::projects::automation(app, cmd, window, cx),
        "expand" | "collapse" if cmd.get("projectRow").is_some() => crate::projects_list::fold_automation(app, cmd, cx),
        "schedule" | "close-schedule" => crate::scheduled::editor_automation(app, cmd, window, cx),
        "archive" | "snooze" if cmd.get("id").is_some() || cmd.get("said").is_some() => crate::sidebar::automation(app, cmd, window, cx),
        "click" if cmd.get("tip").is_some() || cmd.get("tipPrefix").is_some() => crate::sidebar::automation(app, cmd, window, cx),
        // The pane beside the thread (highlights.rs): the Swift app's names.
        "preview" | "folder" | "highlight" | "trust" | "action" | "project-why" => crate::highlights::automation(app, cmd, window, cx),
        "expand" | "collapse" if cmd.get("highlightSection").is_some() => crate::highlights::automation(app, cmd, window, cx),
        "viewer" => {
            let side = app.read(cx).side.clone();
            crate::viewer::viewer_command(&side, cmd, cx)
        }
        // What each picture of the last row of at least `count` says, as the
        // Swift app's `tiles`: "picture" for one that's drawn, else its words
        // (another Mac's, asked of its runner: remote_pictures.rs).
        "tiles" => crate::remote_pictures::tiles_command(app, cmd, cx),
        // Where things are on screen, for a real event there (features/tools/real-gestures.ts).
        "row-point" => crate::row_swipe::row_point_command(app, cmd, window, cx),
        // Where a row's swipe has got to, whatever's moving it: a real
        // trackpad's included (features/tools/real-gestures.ts).
        "row-swipe" => Ok(json!(format!("ok {}", app.read(cx).sidebar.gestures.along()))),
        // Where a control is on screen by its label (a Highlights row's is
        // its title): "ok x y". `prefix`: a label that starts so (a sidebar
        // row's is its title, then a newline). The point is the middle of
        // what's drawn of it, not of its whole bounds: a Highlights card
        // scrolled half out of its row had its middle over the transcript,
        // and a right click there opened the transcript's menu (13 Nov).
        "label-point" => {
            window.render_frame(cx);
            let label = text(cmd, "label");
            let prefix = cmd["prefix"].as_bool() == Some(true);
            let (x, y) = label_point(window, label, prefix)?;
            Ok(json!(format!("ok {x} {y}")))
        }
        // What a screen reader is told of the control labelled so (`prefix`
        // as `label-point`): AccessKit's role and its states, or null.
        "a11y" => {
            window.render_frame(cx);
            let label = text(cmd, "label");
            let prefix = cmd["prefix"].as_bool() == Some(true);
            let found = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| if prefix { l.starts_with(label) } else { l == label })).last().ok_or_else(|| format!("nothing labelled {label:?} on screen"))?;
            let node = json!({
                "role": found.role().map(|r| format!("{r:?}")),
                "selected": found.selected(),
                "checked": found.checked(),
                "disabled": found.disabled(),
            });
            Ok(json!(format!("ok {node}")))
        }
        "tile-point" => crate::remote_pictures::tile_point_command(app, cmd, window, cx),
        // A picture tile of the transcript, as a click on it (`viewer` says what opened).
        "tile" => {
            let index = cmd["index"].as_u64().unwrap_or(0) as usize;
            let count = cmd["count"].as_u64().unwrap_or(1) as usize;
            let images = app.update(cx, |t, cx| t.update_pane(cx, |p, _| p.last_row_images(count)))??;
            crate::viewer::open_from_tile(&images, index, window, cx);
            let side = app.read(cx).side.clone();
            crate::viewer::viewer_command(&side, &json!({}), cx)
        }
        "open" => {
            let id = app.read(cx).thread_id_from(text(cmd, "id"))?;
            app.update(cx, |t, cx| {
                t.reveal_thread(&id, cx);
                t.open(id.clone(), window, cx)
            });
            Ok(json!(format!("open ok {id}")))
        }
        // As the Swift app's: picks it and nothing more, so the sidebar stays
        // on the view it's showing (a schedule's target does this).
        "select" => {
            let id = app.read(cx).thread_id_from(text(cmd, "id"))?;
            app.update(cx, |t, cx| t.open(id.clone(), window, cx));
            Ok(json!(format!("ok {id}")))
        }
        // The window shell (shell.rs, rails.rs, header.rs), the Swift
        // app's commands of the same names.
        "dock-move" | "panel" | "float" | "mode" | "zoom" | "rename" | "undo-archive" | "open-archived" | "error" | "mouse-button" | "copy-thread-id" | "menus" | "menu-item" | "app-menu" | "data-dir" | "share" | "signin" | "edit-message" | "message-words" | "refusal" | "appearance" | "resize" => shell_step(app, cmd, window, cx),
        "search" => {
            let words = text(cmd, "text").to_string();
            let search = app.read(cx).search.clone();
            search.update(cx, |s, cx| s.set_value(words, window, cx));
            Ok(json!("ok"))
        }
        "type" | "send" => {
            type_in_composer(app, text(cmd, "text"), cmd["replace"].as_bool().unwrap_or(false), window, cx)?;
            if text(cmd, "cmd") == "send" {
                window.press("enter", cx);
            }
            Ok(json!("ok"))
        }
        "return" => {
            window.press(
                if cmd["shift"].as_bool().unwrap_or(false) {
                    "shift-enter"
                } else if cmd["option"].as_bool().unwrap_or(false) {
                    "alt-enter"
                } else {
                    "enter"
                },
                cx,
            );
            Ok(json!("ok"))
        }
        // The keys the composer's slash palette takes, where the cursor is
        // (the Swift `arrow`, `tab`, `escape`).
        "arrow" => {
            // With {"settings": true} this is the Settings window's, where its
            // list of sections has the keyboard.
            window.press(if text(cmd, "dir") == "up" { "up" } else { "down" }, cx);
            Ok(json!("ok"))
        }
        "tab" => {
            window.press("tab", cx);
            Ok(json!("ok"))
        }
        "escape" => {
            window.press("escape", cx);
            Ok(json!("ok"))
        }
        // The Swift app's `paste-image` and `drop`: a file through the same
        // route a paste or a drop takes.
        #[cfg(target_os = "macos")]
        "picture-input" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            let path = std::path::PathBuf::from(text(cmd, "path"));
            crate::composer_media::fixture_begin(text(cmd, "kind"), &path)?;
            let result = (|| {
                if cmd["drop"].as_bool().unwrap_or(false) {
                    cx.activate(true);
                    window.activate_window();
                    window.render_frame(cx);
                    let vanish = cmd["vanish"].as_bool().unwrap_or(false).then_some(path.as_path());
                    crate::composer_media::fixture_drag(window, 0, vanish)?;
                } else {
                    let input = composer.read(cx).input.clone();
                    if !cmd["keepFocus"].as_bool().unwrap_or(false) {
                        window.focus(&input.read(cx).focus_handle(cx), cx);
                        crate::web_panels::keyboard_to_app(window);
                    }
                    window.render_frame(cx);
                    window.press("secondary-v", cx);
                }
                Ok::<_, String>(())
            })();
            if !cmd["drop"].as_bool().unwrap_or(false) || result.is_err() { crate::composer_media::fixture_end(); }
            result?;
            Ok(json!("ok"))
        }
        #[cfg(target_os = "macos")]
        "picture-drop" => {
            let path = std::path::PathBuf::from(text(cmd, "vanish"));
            let vanish = (!path.as_os_str().is_empty()).then_some(path.as_path());
            crate::composer_media::fixture_drag(window, cmd["phase"].as_i64().unwrap_or(2) as i32, vanish)?;
            Ok(json!("ok"))
        }
        "paste-image" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            let path = std::path::PathBuf::from(text(cmd, "path"));
            if !crate::composer::is_picture(&path) {
                return Err("not an image".into());
            }
            composer.update(cx, |c, cx| c.take_paths(vec![path], window, cx));
            Ok(json!("ok"))
        }
        "drop" => {
            let mut position = point(px(cmd["x"].as_f64().unwrap_or(0.) as f32), px(cmd["y"].as_f64().unwrap_or(0.) as f32));
            // `boxOffset`: let go just inside the left of that letter of the
            // message box's words, for a drop that goes where it's let go.
            if let Some(offset) = cmd["boxOffset"].as_u64() {
                window.render_frame(cx);
                let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
                let offset = offset as usize;
                let letter = composer.read(cx).input.read(cx).range_to_bounds(&(offset..offset + 1)).ok_or("that letter isn't on screen")?;
                position = point(letter.left() + px(1.), letter.center().y);
            }
            if cmd["leave"].as_bool().unwrap_or(false) {
                window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
                return Ok(json!("ok"));
            }
            // Submit activates the app, and a window that wasn't active drops the
            // first event (only a scripted window starts out that way).
            cx.activate(true);
            window.activate_window();
            window.render_frame(cx);
            let paths = ExternalPaths(vec![std::path::PathBuf::from(text(cmd, "path"))].into());
            window.dispatch_event(FileDropEvent::Entered { position, paths }.to_platform_input(), cx);
            window.render_frame(cx);
            // A real drag moves over the window before it drops. The first frame
            // after Entered still has the old hover state, and the drop's target
            // wants this one.
            window.dispatch_event(FileDropEvent::Pending { position }.to_platform_input(), cx);
            window.render_frame(cx);
            if !cmd["hover"].as_bool().unwrap_or(false) {
                window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
            }
            Ok(json!("ok"))
        }
        // The Swift app's `composer-menu`: a chip's menu as text, or a click
        // on one of its items.
        "composer-menu" => {
            // Other… under a runner's Folder: its dialog, the path typed in
            // and Return pressed there.
            if text(cmd, "menu") == "runner" && text(cmd, "click") == "Other…" {
                let field = app.update(cx, |t, cx| t.type_remote_folder(window, cx));
                if let Some(folder) = cmd["folder"].as_str() {
                    field.update(cx, |f, cx| f.set_value(folder.to_owned(), window, cx));
                    window.render_frame(cx);
                    window.press("enter", cx);
                }
                return Ok(json!("ok"));
            }
            if text(cmd, "menu") == "runner" {
                return crate::new_thread::runner_menu_command(app, cmd, cx);
            }
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            let data = composer.read(cx).chip_data(cx);
            match text(cmd, "menu") {
                "permission" => {
                    let summary = data.summary.clone().unwrap_or_default();
                    if let Some(title) = cmd["click"].as_str() {
                        let Some(mode) = crate::composer_chips::permission_modes(&summary, &data.logins).into_iter().find(|m| threadbear_core::logins::permission_title(m) == title) else { return Err(format!("no item {title}")) };
                        composer.update(cx, |c, cx| c.set_permission(mode, cx));
                        return Ok(json!("ok"));
                    }
                    Ok(json!(crate::composer_chips::permission_lines(&summary, &data.logins)))
                }
                "context" => match data.summary.as_ref().and_then(|t| threadbear_core::composer::ContextMeter::new(t.context_tokens, t.context_window).map(|m| (t.id.clone(), m))) {
                    // `show`: the ring clicked, as a person does, which opens
                    // its popover and asks what fills the context.
                    Some((_, meter)) if cmd["show"].as_bool() == Some(true) => {
                        window.render_frame(cx);
                        let tip = meter.tip();
                        let Some(ring) = window.find_all_by_label(&tip).into_iter().find(|e| e.visible()) else { return Err("no context ring on screen".into()) };
                        let at = ring.visible_bounds().center();
                        window.pointer_move(at, None, cx);
                        window.pointer_down(at, MouseButton::Left, cx);
                        window.pointer_up(at, MouseButton::Left, cx);
                        Ok(json!("ok window 0"))
                    }
                    // Its tokens and percent, then Claude's breakdown once the
                    // runner has answered (composer_context.rs).
                    Some((thread, meter)) => {
                        let mut lines = vec![meter.amount(), format!("{}%", meter.percent())];
                        lines.extend(crate::composer_context::breakdown_lines(&thread));
                        Ok(json!(lines))
                    }
                    None => Err("no context meter on this thread".into()),
                },
                // The "+" menu's folders, and a pick of one.
                "plus" => {
                    let click = cmd["click"].as_str().map(str::to_owned);
                    composer.update(cx, |c, cx| c.plus_command(click.as_deref(), cx))
                }
                // The login picker (composer_account.rs).
                "account" if cmd["show"].as_bool() != Some(true) => crate::composer_account::menu_command(&composer, cmd, cx),
                // `show`: the chip clicked, as a person does, which opens its
                // picker over the box (the Swift command says the popover's
                // window; here it's in the main one).
                menu @ ("account" | "model" | "") if cmd["show"].as_bool() == Some(true) => {
                    let title = crate::composer_chips::chip_title(&data, menu).ok_or("no chip like that here")?;
                    window.render_frame(cx);
                    let Some(chip) = window.find_all_by_label(&title).into_iter().find(|e| e.visible()) else { return Err(format!("no chip {title:?} on screen")) };
                    let at = chip.visible_bounds().center();
                    window.pointer_move(at, None, cx);
                    window.pointer_down(at, MouseButton::Left, cx);
                    window.pointer_up(at, MouseButton::Left, cx);
                    Ok(json!("ok window 0"))
                }
                // Words typed in the open picker's search, then Return there,
                // through the search box's own key handling.
                "model" | "" if cmd["search"].is_string() => crate::composer_chips::search_command(&composer, text(cmd, "search"), window, cx).map(|_| json!("ok")),
                _ => {
                    if let Some(title) = cmd["click"].as_str() {
                        return crate::composer_chips::picker_click(&composer, title, cx).map(|_| json!("ok"));
                    }
                    Ok(json!(crate::composer_chips::picker_lines(&composer, cx)))
                }
            }
        }
        // The open card's picks (the Swift `decide`), and its buttons (`ask`).
        "decide" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            composer.update(cx, |c, cx| c.decide(cmd, window, cx)).map(|s| json!(s))
        }
        "ask" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            composer.update(cx, |c, cx| c.card_action(text(cmd, "action"), window, cx)).map(|_| json!("ok"))
        }
        // The box copied or cut whole, or the clipboard pasted, as ⌘A then ⌘C
        // or ⌘X, and ⌘V; and the chips in it (composer_inline.rs).
        "composer-clip" => crate::composer_inline::clip_command(app, cmd, window, cx),
        // A drag of the slash palette's edge or a column's line (slash_resize.rs).
        "palette-drag" => crate::slash_resize::drag_command(cmd, window, cx),
        "composer-chips" => crate::composer_inline::chips_command(app, cx),
        // The box's spelling marks and its right-click menu (composer_spelling.rs).
        "composer-spelling" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            Ok(crate::composer_spelling::marks_command(&composer, cx))
        }
        "composer-right-click" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            crate::composer_spelling::menu_command(&composer, cmd, window, cx)
        }
        // How many times it's beeped (Return with nowhere to go on the card).
        "beeps" => Ok(json!(format!("ok {}", crate::composer_answers::beeps()))),
        "composer-peek" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            let number = cmd["number"].as_u64().map(|n| n as usize);
            // `"press": true` clicks the chip; `"strip": n` (0-based) the
            // thumbnail over the box, `"remove": true` with it its ×. Says
            // what the viewer was handed, as the Swift app says what Quick
            // Look would show (`ok quicklook <index> of [paths]`).
            let strip = cmd["strip"].as_u64().map(|n| n as usize);
            if cmd["press"].as_bool() == Some(true) || strip.is_some() {
                return crate::composer::Composer::debug_click_picture(&composer, number, strip, cmd["remove"].as_bool() == Some(true), window, cx);
            }
            composer.update(cx, |c, cx| c.debug_peek(number, cx))?;
            Ok(json!(if number.is_some() { "ok window 0" } else { "ok" }))
        }
        // `text` pasted as ⌘V would (the Swift app's `paste`): through the
        // clipboard, put back afterwards so a test machine's isn't left changed.
        "paste" => {
            let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
            let input = composer.read(cx).input.clone();
            let handle = input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
            crate::web_panels::keyboard_to_app(window);
            window.render_frame(cx);
            let saved = cx.read_from_clipboard();
            cx.write_to_clipboard(ClipboardItem::new_string(text(cmd, "text").to_string()));
            window.press("secondary-v", cx);
            if let Some(saved) = saved {
                cx.write_to_clipboard(saved);
            }
            Ok(json!("ok"))
        }
        // Blocks the main thread for `seconds`, to try what a freeze does
        // (hang_watch.rs), as the Swift app's `stall`; `hangs` says what the
        // hang watch noted.
        "stall" => {
            std::thread::sleep(std::time::Duration::from_secs_f64(cmd["seconds"].as_f64().unwrap_or(6.).clamp(0., 60.)));
            Ok(json!("ok"))
        }
        "hangs" => crate::hang_watch::automation(),
        // A panic on a thread of its own, so the app carries on, for what the
        // panic hook keeps (crash_reporting.rs); `crashes` says what it kept.
        "panic" => {
            let message = cmd["message"].as_str().unwrap_or("a scripted panic").to_owned();
            let _ = std::thread::Builder::new().name("scripted panic".into()).spawn(move || panic!("{message}"));
            Ok(json!("ok"))
        }
        "crashes" => crate::crash_reporting::automation(),
        // How smoothly it's been drawing (frame_stats.rs), in a build with
        // `frame-stats`; `reset` starts the count again from now.
        "frame-stats" => {
            #[cfg(feature = "frame-stats")]
            {
                Ok(crate::frame_stats::report(window, cmd["reset"].as_bool().unwrap_or(false)))
            }
            #[cfg(not(feature = "frame-stats"))]
            {
                Err("this build can't measure frames: build it with --features frame-stats".into())
            }
        }
        // When hovers and presses were handled and the frame after each
        // painted (input_probe.rs): `start` records from now, then a bare
        // one hands it back.
        "input-probe" => Ok(crate::input_probe::report(cmd["start"].as_bool().unwrap_or(false), cx)),
        "menu-key" => {
            let mut key = String::new();
            for m in cmd["modifiers"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                key.push_str(match m {
                    // ⌘ on the Mac, Control elsewhere.
                    "cmd" => "secondary-",
                    "ctrl" => "ctrl-",
                    "opt" => "alt-",
                    "shift" => "shift-",
                    _ => "",
                });
            }
            key.push_str(text(cmd, "key"));
            window.press(&key, cx);
            Ok(json!("ok"))
        }
        "click" => {
            let title = text(cmd, "title");
            window.render_frame(cx);
            let target = window.find_all_by_label(title).into_iter().find(|e| e.visible());
            let Some(target) = target else { return Err(format!("no button called {title:?}")) };
            let at = target.visible_bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Left, cx);
            window.pointer_up(at, MouseButton::Left, cx);
            Ok(json!("click ok"))
        }
        // A click at a point in the window, in points from its top left,
        // through its hit testing as a real one goes, without moving the
        // pointer: for what has no label to find it by (the Swift command).
        // `"right": true` for the right button, `count` 2 for a double-click.
        "click-at" => {
            let (Some(x), Some(y)) = (cmd["x"].as_f64(), cmd["y"].as_f64()) else { return Err("needs x and y".into()) };
            window.render_frame(cx);
            let at = point(px(x as f32), px(y as f32));
            let size = window.viewport_size();
            if at.x < px(0.) || at.y < px(0.) || at.x > size.width || at.y > size.height {
                return Err(format!("({x}, {y}) is outside the window ({} by {})", f32::from(size.width), f32::from(size.height)));
            }
            let button = if cmd["right"].as_bool() == Some(true) { MouseButton::Right } else { MouseButton::Left };
            window.pointer_move(at, None, cx);
            // Each press carries its count, as a real double-click's second does.
            for click_count in 1..=cmd["count"].as_u64().unwrap_or(1).clamp(1, 3) as usize {
                let modifiers = window.modifiers();
                window.dispatch_event(MouseDownEvent { button, position: at, modifiers, click_count, first_mouse: false }.to_platform_input(), cx);
                window.render_frame(cx);
                window.dispatch_event(MouseUpEvent { button, position: at, modifiers, click_count }.to_platform_input(), cx);
                window.render_frame(cx);
            }
            Ok(json!("click-at ok"))
        }
        "expand" | "collapse" => {
            let open = text(cmd, "cmd") == "expand";
            let row = text(cmd, "row").to_string();
            let file = (!cmd["file"].is_null()).then(|| cmd["file"].clone());
            let all = cmd["all"].as_bool().unwrap_or(false);
            if file.is_some() || all {
                app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.expand_detail(&row, file.as_ref(), all, open, cx)))??;
            } else {
                app.update(cx, |t, cx| t.set_row_open(&row, open, cx))?;
            }
            Ok(json!("ok"))
        }
        "scroll" | "scroll-to" => {
            let (to, row, offset) = (cmd["to"].as_str().map(str::to_owned), cmd["row"].as_str().map(str::to_owned), cmd["offset"].as_f64().unwrap_or(0.) as f32);
            app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.scroll_command(to.as_deref(), row.as_deref(), offset, cx)))??;
            Ok(json!("ok"))
        }
        "bottom" => {
            app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.scroll_command(Some("bottom"), None, 0., cx)))??;
            Ok(json!("ok"))
        }
        "link" => {
            let (row, url) = (text(cmd, "row").to_string(), text(cmd, "url").to_string());
            app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.follow_link_command(&row, &url, window, cx)))??;
            Ok(json!("ok"))
        }
        "archive" => {
            app.update(cx, |t, cx| t.toggle_archive(window, cx));
            Ok(json!("ok"))
        }
        "snooze" => {
            let seconds = cmd["seconds"].as_i64().unwrap_or(3600);
            app.update(cx, |t, cx| t.snooze_open(seconds, cx))?;
            Ok(json!("ok"))
        }
        "menu" => {
            let (row, click) = (text(cmd, "row").to_string(), cmd["click"].as_str().map(str::to_owned));
            let result = app.update(cx, |t, cx| t.update_pane(cx, |p, cx| p.menu_command(&row, click.as_deref(), cx)))??;
            Ok(json!(result))
        }
        // {"cmd":"press-button","id":"b7k2abcd"} (or "title":"Fill in the
        // form") clicks a reply button in the open thread as its pill would.
        "press-button" => {
            let (thread, ids) = app.read(cx).pane.as_ref().map(|p| (p.read(cx).summary.id.clone(), p.read(cx).linked_buttons())).unwrap_or_default();
            let id = cmd["id"].as_str().and_then(|want| ids.iter().rev().find(|id| *id == want).cloned()).or_else(|| {
                cmd["title"].as_str().and_then(|want| ids.iter().rev().find(|id| crate::reply_buttons::known(&thread, id).is_some_and(|b| b.title == want)).cloned())
            });
            let Some(id) = id else { return Err("no reply button like that in the open thread".into()) };
            if crate::reply_buttons::known(&thread, &id).is_none() {
                return Err(format!("button {id} isn't known"));
            }
            app.update(cx, |t, cx| t.press_reply_button(thread, id, window, cx));
            Ok(json!("ok"))
        }
        "clipboard" => {
            // `"set": words` puts them there first, as Voice Type does before its ⌘V.
            if let Some(words) = cmd["set"].as_str() {
                cx.write_to_clipboard(ClipboardItem::new_string(words.to_string()));
            }
            // `"type": "html"`: the HTML beside the words, as a draft's Copy puts it.
            let words = if cmd["type"] == "html" { crate::draft_boxes::clipboard_html().unwrap_or_default() } else { cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default() };
            Ok(json!(format!("ok {}", serde_json::to_string(&words).unwrap_or_default())))
        }
        // A draft box's Copy in a reply (draft_boxes.rs), as the Swift app's `draft`.
        "draft" => crate::draft_boxes::automation(app, cmd, window, cx),
        // A reply's links and their cards and menus (reply_links.rs), else the
        // chips in Mike's messages and their cards (chips.rs), as the Swift
        // app's `peek`.
        "peek" => crate::reply_links::peek_command(app, cmd, window, cx).unwrap_or_else(|| crate::chips::peek_command(app, cmd, window, cx)),
        // Words selected in a reply, and the bar over them (selection_bar.rs),
        // as the Swift app's; a follow-up in the Explain pane (explain_pane.rs).
        "select-text" => crate::selection_bar::select_command(app, cmd, window, cx),
        "text-selection-source" => Ok(json!({"words":crate::text_services::front_selection(window,cx).map(|(words,_)|words),"terminalFocused":crate::terminal::has_focus(window,cx)})),
        "text-lines" => crate::selection_bar::text_lines_command(app, cmd, window, cx),
        "selection" => crate::selection_bar::selection_command(app, window, cx),
        "explain" => crate::explain_pane::explain_command(app, cmd, window, cx),
        // What the open gate would have opened, as a scripted app opens nothing.
        "opened" => Ok(json!(format!("ok {}", serde_json::to_string(&crate::open_gate::take_opened()).unwrap_or_default()))),
        "milestone" => Ok(crate::milestones::automation(app, cmd, cx)),
        // Thread chips in a row's words, as the Swift app's `chip`.
        "chip" => crate::chips::chip_command(app, cmd, window, cx),
        // Quits as the app menu's Quit does once it's said yes (the quit
        // handlers run, the shells' among them), after this answer is sent,
        // as the Swift app's `quit` does.
        "quit" => {
            cx.spawn(async |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
            Ok(json!("ok"))
        }
        // As if the window had come to the front: both apps read a thread
        // only while it is, and a scripted one never is.
        "focus" => {
            let active = cmd["on"].as_bool().unwrap_or(true);
            if !active {
                crate::thread::set_looking(crate::thread::window_key(window), false);
            }
            app.update(cx, |t, cx| {
                t.looking_changed(crate::thread::SCRIPTED, active, cx);
            });
            Ok(json!("ok"))
        }
        // Everything the thread has, not just its newest page, then how long a
        // frame takes to draw while it's scrolled (the risk check in stage 0).
        // The open thread's read and the threads kept loaded, without the
        // rows a dump carries, so asking often doesn't slow what it times
        // (features/tools/switch-perf.ts).
        "pane-timing" => {
            let t = app.read(cx);
            let pane = t.pane.as_ref().map(|p| p.read(cx));
            Ok(json!({
                "threadId": pane.map(|p| p.summary.id.clone()),
                "state": pane.map(|p| p.load_state()),
                "rows": pane.map(|p| p.feed.model().rows().len()),
                "timing": pane.map(|p| p.timing_json()),
                "kept": t.kept.ids(cx),
            }))
        }
        "load-all" => {
            app.update(cx, |t, cx| t.load_all_events(cx));
            Ok(json!("ok"))
        }
        "page-scroll" => {
            let (steps, dy) = (cmd["steps"].as_u64().unwrap_or(120) as usize, cmd["dy"].as_f64().unwrap_or(14.) as f32);
            let at = window.viewport_size().map(|v| v / 2.);
            let at = point(at.width, at.height);
            let mut frames = Vec::with_capacity(steps);
            for i in 0..steps {
                let sign = if i < steps / 2 { -1. } else { 1. };
                let event = ScrollEvent { position: at, delta: ScrollDelta::Pixels(point(px(0.), px(dy * sign))), modifiers: Modifiers::default(), touch_phase: TouchPhase::Moved, source: ScrollSource::Pointer };
                window.dispatch_event(event.to_platform_input(), cx);
                let started = std::time::Instant::now();
                window.render_frame(cx);
                frames.push(started.elapsed().as_secs_f64() * 1000.);
            }
            frames.sort_by(|a, b| a.total_cmp(b));
            let at = |p: f64| frames[((frames.len() - 1) as f64 * p) as usize];
            Ok(json!({ "frames": frames.len(), "medianMs": at(0.5), "p95Ms": at(0.95), "worstMs": at(1.0) }))
        }
        // The colour space the window's drawn in (colour_space.rs): "sRGB" once tagged.
        "colour-space" => Ok(json!(crate::colour_space::describe(window))),
        other => Err(format!("unknown command {other:?}")),
    }
}

/// The window shell's commands, named and shaped as the Swift app's
/// (app/README.md, "Seeing and driving it from a script").
fn shell_step(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    use crate::shell;
    use threadbear_dock::{DockDrop, DockEdge};
    let dock_summary = |app: &Entity<Threadbear>, cx: &App| app.read(cx).dock.read(cx).layout().summary().replace("threadbear.", "");
    match text(cmd, "cmd") {
        "dock-move" => {
            let name = cmd["panel"].as_str().ok_or("panel is a panel id")?.to_string();
            let edge = match cmd["edge"].as_str().unwrap_or("right") {
                "left" => DockEdge::Left,
                "right" => DockEdge::Right,
                "top" => DockEdge::Top,
                "bottom" => DockEdge::Bottom,
                _ => return Err("edge is left, right, top or bottom".into()),
            };
            let panels = app.read(cx).dock.read(cx).layout().panels();
            let panel = panels.into_iter().find(|p| *p == name || *p == format!("threadbear.{name}")).ok_or_else(|| format!("{name} isn't in the layout"))?;
            let model = app.read(cx).dock.clone();
            model.update(cx, |m, cx| m.update_controller(cx, |c| c.layout.move_panel(&panel, DockDrop::window(edge))));
            Ok(json!(format!("ok {}", dock_summary(app, cx))))
        }
        "panel" => {
            app.update(cx, |t, cx| {
                let Some(thread) = t.open_thread().cloned() else { return Err("no thread selected".to_string()) };
                if let Some(id) = cmd["id"].as_str() {
                    let mut ids: Vec<String> = vec![shell::HIGHLIGHTS.to_string(), shell::FILES.to_string()];
                    if t.browser_rail(&thread.id, cx).is_some() {
                        ids.push(shell::BROWSER.to_string());
                    }
                    ids.extend(thread.panels.iter().flatten().filter_map(|p| p["id"].as_str().map(String::from)));
                    if thread.patches.as_ref().is_some_and(|p| !p.is_empty()) {
                        ids.push(shell::PATCHES.to_string());
                    }
                    if !ids.iter().any(|i| i == id) {
                        return Err(format!("no panel {id}; this thread has {}", ids.join(", ")));
                    }
                    t.show_pane(id, cx);
                } else if let Some(show) = cmd["show"].as_bool() {
                    let shown = t.dock.read(cx).controller.is_showing(shell::HIGHLIGHTS);
                    if show != shown {
                        t.click_rail(shell::HIGHLIGHTS, cx);
                    }
                } else {
                    t.click_rail(shell::HIGHLIGHTS, cx);
                }
                Ok(())
            })?;
            // What the pane shows, straight away rather than at the next frame, and
            // only now that the app isn't held, because the pane reads it.
            if let (Some(id), Some(thread)) = (cmd["id"].as_str(), app.read(cx).open_thread().cloned()) {
                let side = app.read(cx).side.clone();
                side.update(cx, |pane, cx| pane.choose_section(&thread.id, id, cx));
            }
            let showing = app.read(cx).dock.read(cx).controller.is_showing(cmd["id"].as_str().unwrap_or(shell::HIGHLIGHTS));
            Ok(json!(if showing { "ok showing" } else { "ok hidden" }))
        }
        "float" => {
            let id = text(cmd, "id").to_string();
            let model = app.read(cx).dock.clone();
            if cmd["back"].as_bool() == Some(true) {
                model.update(cx, |m, cx| m.put_back(&id, cx));
            } else {
                if !model.read(cx).layout().can_float(&id) {
                    return Err(format!("{id} can't go into a window of its own (not in the dock, or the last panel there)"));
                }
                model.update(cx, |m, cx| m.open_in_window(&id, cx));
            }
            Ok(json!(format!("ok {}", dock_summary(app, cx))))
        }
        "mode" => {
            let mode = match cmd["mode"].as_str().unwrap_or("inbox") {
                "scheduled" => shell::SidebarMode::Scheduled,
                "projects" => shell::SidebarMode::Projects,
                "librarian" => shell::SidebarMode::Librarian,
                "archive" => {
                    app.update(cx, |t, cx| t.run_command(shell::Command::ShowArchived, window, cx));
                    return Ok(json!("ok"));
                }
                "snoozed" => {
                    app.update(cx, |t, cx| t.run_command(shell::Command::ShowHibernating, window, cx));
                    return Ok(json!("ok"));
                }
                _ => shell::SidebarMode::Inbox,
            };
            app.update(cx, |t, cx| t.set_mode(mode, cx));
            Ok(json!("ok"))
        }
        "zoom" => {
            use threadbear_core::zoom;
            let current = crate::style::zoom_percent();
            let next = match cmd["to"].as_str() {
                Some("in") => zoom::zoom_in(current),
                Some("out") => zoom::zoom_out(current),
                Some("reset") => zoom::DEFAULT_PERCENT,
                Some(_) => return Err("to is in, out or reset".into()),
                None => match cmd["percent"].as_u64() {
                    Some(p) if zoom::STEPS.contains(&(p as u32)) => p as u32,
                    Some(_) => return Err(format!("percent is one of {:?}", zoom::STEPS)),
                    None => current,
                },
            };
            app.update(cx, |t, cx| t.set_zoom(next, window, cx));
            Ok(json!(format!("ok {}", crate::style::zoom_percent())))
        }
        "rename" => {
            let Some(id) = app.read(cx).selected.clone() else { return Err("no thread open".into()) };
            if let Some(title) = cmd["title"].as_str() {
                let title = title.to_string();
                app.update(cx, |t, cx| {
                    t.finish_rename(false, cx);
                    t.rename_thread(&id, &title, cx);
                });
            } else {
                app.update(cx, |t, cx| t.start_renaming(id, window, cx));
            }
            Ok(json!("ok"))
        }
        "undo-archive" | "open-archived" => {
            let undo = text(cmd, "cmd") == "undo-archive";
            let Some(note) = app.read(cx).archived_note.clone() else { return Err("no archived note".into()) };
            app.update(cx, |t, cx| if undo { t.undo_archive(window, cx) } else { t.open_archived_note(window, cx) });
            Ok(json!(format!("ok {}", note.thread)))
        }
        "error" => {
            let message = cmd["text"].as_str().map(str::to_owned);
            app.update(cx, |t, cx| {
                t.error_message = message;
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "mouse-button" => {
            let back = match cmd["button"].as_str() {
                Some("back") => true,
                Some("forward") => false,
                _ => return Err("button is back or forward".into()),
            };
            app.update(cx, |t, cx| if back { t.go_back(window, cx) } else { t.go_forward(window, cx) });
            let t = app.read(cx);
            Ok(json!(format!("ok {} {} back={} forward={}", t.mode.name(), t.selected.clone().unwrap_or_else(|| "new".into()), t.can_go_back(), t.can_go_forward())))
        }
        "copy-thread-id" => {
            let Some(id) = app.read(cx).selected.clone() else { return Err("no thread open".into()) };
            cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
            Ok(json!(format!("ok {id}")))
        }
        "menus" => Ok(json!(format!("ok {}", serde_json::to_string(&app.read(cx).menus_for_dump(cx)).unwrap_or_default()))),
        "app-menu" => {
            let menus = app.read(cx).menus_for_dump(cx);
            let lines: Vec<String> = menus[0]["items"].as_array().into_iter().flatten().map(|i| {
                let title = i["title"].as_str().unwrap_or("");
                i["key"].as_str().map_or(title.to_string(), |k| format!("{title} [{k}]"))
            }).collect();
            Ok(json!(format!("ok {}", serde_json::to_string(&lines).unwrap_or_default())))
        }
        "menu-item" => {
            let title = cmd["title"].as_str().ok_or("title missing")?.to_string();
            app.update(cx, |t, cx| t.run_menu_item(&title, window, cx))?;
            Ok(json!("ok"))
        }
        "data-dir" => Ok(json!(format!("ok {}", app.read(cx).settings.path().parent().map(|p| p.display().to_string()).unwrap_or_default()))),
        "share" => {
            let Some(id) = app.read(cx).selected.clone() else { return Err("no thread is open".into()) };
            if let Some(access) = cmd["access"].as_str() {
                if access != "off" && access != "link" {
                    return Err("access is off or link".into());
                }
                let access = access.to_string();
                let id2 = id.clone();
                app.update(cx, |t, cx| t.set_share(id2, &access, false, cx));
            }
            if let Some(open) = cmd["open"].as_bool() {
                app.update(cx, |t, cx| {
                    t.share_popover = open.then(|| id.clone());
                    cx.notify();
                });
            }
            Ok(json!(app.read(cx).share_status()))
        }
        // The words one of Mike's messages draws in its bubble ({"row": its
        // id or start}), from what the bubble lays out (rows.rs's
        // `answered_words`): "ok a | b", its lines joined, as the Swift
        // app's. For what a bubble shows rather than what its message holds;
        // a card's answer dropped his words.
        "message-words" => {
            let prefix = text(cmd, "row").to_owned();
            let pane = app.read(cx).pane.clone().ok_or("no thread open")?;
            let row = pane.read(cx).feed.model().rows().iter().find(|r| r.id.starts_with(&prefix)).cloned().ok_or_else(|| format!("no row starting {prefix:?}"))?;
            let threadbear_core::reducer::RowContent::User(message) = &row.content else { return Err(format!("{} isn't a message", row.id)) };
            let words: Vec<String> = match crate::rows::answered_words(message) {
                Some(words) => words.lines.iter().flat_map(|(label, text)| [label.clone(), text.clone()]).chain([words.extra]).collect(),
                None => vec![message.text.clone()],
            };
            let lines: Vec<&str> = words.iter().flat_map(|w| w.lines()).filter(|l| !l.is_empty()).collect();
            Ok(json!(format!("ok {}", lines.join(" | "))))
        }
        // The pencil by one of Mike's messages, as a click on it does:
        // {"row": the row's id or its start}. `"click": true` clicks the
        // pencil itself through the window, the pointer over it first as
        // it shows on hover, so a pencil that's drawn but doesn't answer
        // fails (thread 3fdaaeec, 12 Nov), as calling the handler can't.
        "edit-message" => {
            let prefix = text(cmd, "row").to_owned();
            let pane = app.read(cx).pane.clone().ok_or("no thread open")?;
            let (ix, row) = pane.read(cx).feed.model().rows().iter().enumerate().find(|(_, r)| r.id.starts_with(&prefix)).map(|(ix, r)| (ix, r.id.clone())).ok_or_else(|| format!("no row starting {prefix:?}"))?;
            if cmd["click"].as_bool() == Some(true) {
                pane.update(cx, |p, cx| p.scroll_command(None, Some(&row), 0., cx))?;
                window.render_frame(cx);
                let pencil = window.find_all(("edit-message", ix)).into_iter().next().ok_or("no pencil by that message")?;
                // Its whole middle, not `click_point`: the pencil is drawn
                // only while its row is hovered, so it isn't visible until
                // this move reveals it.
                let at = pencil.bounds().center();
                window.pointer_move(at, None, cx);
                window.render_frame(cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
            } else {
                pane.update(cx, |p, cx| p.edit_message(&row, window, cx));
            }
            let editing = app.read(cx).composer(cx).is_some_and(|c| c.read(cx).editing.is_some());
            Ok(json!(if editing { "ok editing" } else { "error that message can't be edited" }))
        }
        "signin" => {
            if cmd["sendAgain"].as_bool() == Some(true) {
                let status = app.read(cx).signin_status(cx);
                if status != "ok none" {
                    let id = app.read(cx).selected.clone().unwrap_or_default();
                    app.update(cx, |t, cx| t.send_again(&id, cx));
                    return Ok(json!(format!("ok sent again for {}", status.trim_start_matches("ok "))));
                }
            }
            Ok(json!(app.read(cx).signin_status(cx)))
        }
        "refusal" => {
            if cmd["continue"].as_bool() == Some(true) {
                let target = app.read(cx).open_thread().and_then(|t| app.read(cx).refusal_target(t, cx));
                if let Some((account, _)) = target {
                    let thread = app.read(cx).selected.clone().unwrap_or_default();
                    app.update(cx, |t, cx| t.continue_refusal(thread, account, cx));
                }
            }
            Ok(json!(app.read(cx).refusal_status(cx)))
        }
        // `appearance`: the window's light or dark look, as the Swift command
        // sets NSApp.appearance. Lasts until the system changes.
        "appearance" => {
            let mode = cmd["mode"].as_str().ok_or("mode is light, dark or system")?;
            // The preference too, so the Mac's own change doesn't undo it; a
            // script's choice isn't saved.
            match mode {
                "light" | "dark" | "system" => crate::style::set_appearance(crate::style::Appearance::from_name(mode), window, cx),
                other => return Err(format!("mode is light, dark or system, not {other:?}")),
            }
            Ok(json!(format!("ok {mode}")))
        }
        // `resize`: the window's content size, as the Swift command sets it
        // (for pictures at a given size).
        "resize" | "size" => {
            let (w, h) = (cmd["w"].as_f64().unwrap_or(1240.) as f32, cmd["h"].as_f64().unwrap_or(820.) as f32);
            window.resize(size(px(w), px(h)));
            Ok(json!(format!("ok {w}x{h}")))
        }
        other => Err(format!("unknown shell command {other:?}")),
    }
}

fn type_in_composer(app: &Entity<Threadbear>, words: &str, replace: bool, window: &mut Window, cx: &mut App) -> Result<(), String> {
    // On the new-thread page the box is made at its first render; a script
    // may type before that frame has been drawn.
    if app.read(cx).pane.is_none() {
        app.update(cx, |t, cx| t.ensure_new_composer(window, cx));
    }
    let Some(composer) = app.read(cx).composer(cx) else { return Err("no thread open".into()) };
    let input = composer.read(cx).input.clone();
    if replace {
        input.update(cx, |s, cx| s.set_value("", window, cx));
    }
    let handle = input.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
    crate::web_panels::keyboard_to_app(window);
    window.render_frame(cx);
    window.input(words, cx);
    Ok(())
}

/// label-point against a Highlights card scrolled half out of its pane, with
/// the transcript beside it: the card's whole-bounds middle is over the
/// transcript, so a right click there opened the transcript's menu (13 Nov).
#[cfg(test)]
mod label_point_tests {
    use super::label_point;
    use gpui_kit::prelude::*;
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    // By name: a glob of the kit would bring its `test` over the one the
    // test macro's own expansion uses.
    use gpui_kit::{div, px, size, Bounds, MouseButton, Point, TestAppContext, TestSupportExt as _, Window, WindowBounds, WindowOptions};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Board {
        transcript: Rc<Cell<u32>>,
        card: Rc<Cell<u32>>,
    }

    impl Render for Board {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (transcript, card) = (self.transcript.clone(), self.card.clone());
            div()
                .size_full()
                .flex()
                .flex_row()
                // The transcript, 0 to 100.
                .child(div().id("transcript").w(px(100.)).h_full().on_mouse_down(MouseButton::Right, move |_, _, _| transcript.set(transcript.get() + 1)))
                // The pane, 100 to 300, clipping a card scrolled so it spans
                // -50 to 150: 100 to 150 shows. And one scrolled wholly out.
                .child(
                    div()
                        .id("pane")
                        .relative()
                        .w(px(200.))
                        .h_full()
                        .overflow_hidden()
                        .child(div().id("card").test_support().aria_label("Card").absolute().left(px(-150.)).top(px(10.)).w(px(200.)).h(px(40.)).on_mouse_down(MouseButton::Right, move |_, _, _| card.set(card.get() + 1)))
                        .child(div().id("gone").test_support().aria_label("Gone").absolute().left(px(-300.)).top(px(60.)).w(px(100.)).h(px(20.))),
                )
        }
    }

    #[gpui_kit::test]
    fn a_partly_scrolled_out_card_is_clicked_where_it_shows(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let transcript = Rc::new(Cell::new(0));
        let card = Rc::new(Cell::new(0));
        let (t, c) = (transcript.clone(), card.clone());
        let window = cx.update(|cx| {
            let bounds = Bounds { origin: Point::default(), size: size(px(300.), px(100.)) };
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
            gpui_kit::open_window(options, cx, move |_, cx| cx.new(|_| Board { transcript: t, card: c })).expect("a test window").0
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let drawn = window.find_all_by_label("Card").into_iter().next().expect("the card");
            assert!(drawn.visible());
            assert!(drawn.bounds().center().x < px(100.), "the test needs the card's whole middle over the transcript: {:?}", drawn.bounds());
            let (x, y) = label_point(window, "Card", false).expect("the card's point");
            assert!(x > 100 && x < 150, "{x} isn't on the drawn part of the card");
            let at = gpui_kit::point(px(x as f32), px(y as f32));
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Right, cx);
            window.pointer_up(at, MouseButton::Right, cx);
            assert_eq!((card.get(), transcript.get()), (1, 0), "the right click went to the transcript, not the card");
            assert!(label_point(window, "Gone", false).is_err(), "a card wholly out of sight has no point");
            // A sliver at a pane's edge: its rounded middle stays in it.
            let sliver = |left: f32, right: f32| super::whole_point(Bounds::from_corners(gpui_kit::point(px(left), px(0.)), gpui_kit::point(px(right), px(10.)))).map(|(x, _)| x);
            assert_eq!(sliver(299.6, 300.), None, "no whole point is inside 299.6 to 300");
            assert_eq!(sliver(298.6, 300.), Some(299));
            assert_eq!(sliver(100., 150.), Some(125));
        })
        .expect("the test window");
    }
}
