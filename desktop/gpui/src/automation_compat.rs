// The remaining Swift script commands. These exist only in automation
// builds, with the same private clipboard and window-only input as the rest.
use crate::app::Threadbear;
use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
use gpui_kit::*;
use serde_json::{json, Value};

pub(crate) fn step(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    match cmd["cmd"].as_str().unwrap_or("") {
        "account-ids" => {
            app.update(cx, |app, cx| {
                for account in &mut app.logins.accounts {
                    account.title = account.id.clone();
                    account.email = None;
                    account.note = None;
                    account.usage = None;
                }
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "room" => {
            let percent = cmd["percent"].as_f64().filter(|p| p.is_finite() && (0.0..=100.0).contains(p)).ok_or("percent must be from 0 to 100")?;
            app.update(cx, |app, cx| {
                for account in &mut app.logins.accounts {
                    account.usage = Some(json!({ "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": percent }] }));
                }
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "streaming" => {
            let pane = app.read(cx).pane.as_ref().ok_or("no thread open")?.read(cx);
            let mut streaming = serde_json::Map::new();
            for row in pane.feed.model().rows() {
                match &row.content {
                    threadbear_core::reducer::RowContent::Assistant(text) | threadbear_core::reducer::RowContent::Reasoning(text) if text.is_streaming => {
                        streaming.insert(text.item_id.clone(), json!(text.text));
                    }
                    _ => {}
                }
            }
            Ok(json!(format!("ok {}", Value::Object(streaming))))
        }
        "debug-info" => {
            let pane = app.read(cx).pane.as_ref().ok_or("no thread open")?.read(cx);
            let prefix = cmd["row"].as_str().ok_or("give a row id or its start")?;
            let row = pane.feed.model().rows().iter().find(|r| r.id.starts_with(prefix)).ok_or("no such row")?;
            let info = pane.debug_info(row);
            cx.write_to_clipboard(ClipboardItem::new_string(info.clone()));
            Ok(json!(format!("ok {}", json!(info))))
        }
        "window" => {
            window.render_frame(cx);
            let size = window.viewport_size();
            Ok(json!(format!("ok {} {}x{}", window_number(window), f32::from(size.width) as i32, f32::from(size.height) as i32)))
        }
        "runners-front" => {
            crate::runners_panel::automation(app, &json!({ "show": true }), window, cx)?;
            #[cfg(target_os = "macos")]
            if let Some(native) = crate::window_menu::ns_window(window) {
                native.setLevel(objc2_app_kit::NSFloatingWindowLevel);
            }
            cx.activate(true);
            window.activate_window();
            window.render_frame(cx);
            let bounds = window.window_bounds().get_bounds();
            Ok(json!(format!("ok {} {} {}", window_number(window), f32::from(bounds.origin.x) as i32, f32::from(bounds.origin.y) as i32)))
        }
        "point" => point_command(cmd, window, cx),
        other => Err(format!("unknown compatibility command {other}")),
    }
}

fn window_number(window: &Window) -> u64 {
    #[cfg(target_os = "macos")]
    if let Some(native) = crate::window_menu::ns_window(window) { return native.windowNumber() as u64; }
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        match handle.as_raw() {
            RawWindowHandle::Xlib(h) => return h.window as u64,
            RawWindowHandle::Xcb(h) => return h.window.get() as u64,
            RawWindowHandle::Win32(h) => return h.hwnd.get() as u64,
            _ => {}
        }
    }
    window.window_handle().window_id().as_u64()
}

fn point_command(cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    window.render_frame(cx);
    let size = window.viewport_size();
    let point_at = |x: f64, y: f64| -> Result<Point<Pixels>, String> {
        let at = point(px(x as f32), px(y as f32));
        if !x.is_finite() || !y.is_finite() || x < 0. || y < 0. || at.x > size.width || at.y > size.height { return Err("point is outside the window".into()); }
        Ok(at)
    };
    let x = cmd["x"].as_f64().ok_or("point needs x and y")?;
    let y = cmd["y"].as_f64().ok_or("point needs x and y")?;
    let at = point_at(x, y)?;
    let to = if cmd.get("toX").is_some() || cmd.get("toY").is_some() {
        // Validate both ends before pressing anything.
        Some(point_at(cmd["toX"].as_f64().unwrap_or(x), cmd["toY"].as_f64().unwrap_or(y))?)
    } else { None };
    if cmd.get("scrollX").is_some() || cmd.get("scrollY").is_some() {
        let event = ScrollEvent { position: at, delta: ScrollDelta::Lines(point(cmd["scrollX"].as_f64().unwrap_or(0.) as f32, cmd["scrollY"].as_f64().unwrap_or(0.) as f32)), modifiers: Modifiers::default(), touch_phase: TouchPhase::Moved, source: ScrollSource::Pointer };
        window.dispatch_event(event.to_platform_input(), cx);
    } else {
        window.pointer_move(at, None, cx);
        window.pointer_down(at, MouseButton::Left, cx);
        if let Some(to) = to {
            for n in 1..=20 {
                let t = n as f32 / 20.;
                window.pointer_move(at + (to - at) * t, Some(MouseButton::Left), cx);
                window.render_frame(cx);
            }
        }
        if !cmd["holdMs"].as_f64().is_some_and(|hold| hold.is_finite() && hold > 0.) {
            window.pointer_up(to.unwrap_or(at), MouseButton::Left, cx);
        }
    }
    window.render_frame(cx);
    Ok(json!("ok"))
}
