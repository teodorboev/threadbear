// Right-clicking Send to send the message later, as Mail and Slack do
// (ComposerControls.swift, the `iconContextMenu` on SendButton): Schedule…
// opens the schedule editor with what's in the box, as "+" > Schedule… does.
// Not on another Mac's thread, or a new one for another Mac: their schedules
// are made from there.
use crate::composer::Composer;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Its one item, as the Swift menu names it.
pub(crate) const SCHEDULE_TITLE: &str = "Schedule…";

/// What the send button's states are called to VoiceOver and to a script
/// (SendButton's accessibility labels).
#[cfg(feature = "automation")]
const SEND_BUTTON_NAMES: [&str; 4] = ["Send", "Stop", "Stopping", "Starting the thread"];

impl Composer {
    /// Whether Send's right-click has Schedule… in it: the runner keeps
    /// schedules, and the box's thread (or new one) is on this Mac's.
    pub(crate) fn schedules_from_send(&self, cx: &App) -> bool {
        self.chip_data(cx).away.is_none() && self.app.upgrade().is_some_and(|a| a.read(cx).schedules_supported)
    }

    /// The send button (or Stop, or a spinner, as it stands) with its
    /// right-click menu, when there's something for it to offer.
    pub(crate) fn with_send_menu(&self, button: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if !self.schedules_from_send(cx) {
            return button;
        }
        let composer = cx.entity().downgrade();
        div()
            .id("composer-send-menu")
            .flex_none()
            .child(button)
            .context_menu(move |menu, _, _| {
                let composer = composer.clone();
                menu.item(PopupMenuItem::new(SCHEDULE_TITLE).icon(crate::icons::symbol("clock")).on_click(move |_, window, cx| {
                    let Some(composer) = composer.upgrade() else { return };
                    let Some(app) = composer.read(cx).app.upgrade() else { return };
                    app.update(cx, |app, cx| app.new_schedule_from_composer(&composer, window, cx));
                }))
            })
            .into_any_element()
    }
}

/// The automation's `composer-menu` for `send` (both apps): a right-click on
/// the send button through real events and hit testing, then, once the kit
/// has built the menu, its items read back, or one of them pressed with
/// `click`. A command's turn each, as each waits on the one before to be drawn.
#[cfg(feature = "automation")]
pub(crate) async fn menu_command(window: AnyWindowHandle, cmd: &serde_json::Value, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    use serde_json::json;
    let pressed = cx
        .update_window(window, |_, window, cx| {
            crate::thread::scripted_input(|| {
                window.render_frame(cx);
                // The send button, Stop or the spinner: whichever is showing,
                // by the name each has for VoiceOver.
                let found = window.elements().into_iter().find(|e| e.visible() && e.label().is_some_and(|l| SEND_BUTTON_NAMES.contains(&l)));
                let Some(button) = found else { return Err("no send button on screen".to_owned()) };
                let at = button.bounds().center();
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Right, cx);
                window.pointer_up(at, MouseButton::Right, cx);
                Ok(())
            })
        })
        .map_err(|e| e.to_string())??;
    let () = pressed;
    cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
    let items = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            crate::reply_links::open_menu_items(window, 0)
        })
        .map_err(|e| e.to_string())?;
    if let Some(title) = cmd["click"].as_str() {
        let Some((_, item)) = items.iter().find(|(t, _)| t == title) else { return Err(format!("no item {title} in {items:?}")) };
        let at = item.center();
        cx.update_window(window, |_, window, cx| {
            crate::thread::scripted_input(|| {
                window.pointer_move(at, None, cx);
                window.pointer_down(at, MouseButton::Left, cx);
                window.pointer_up(at, MouseButton::Left, cx);
            })
        })
        .map_err(|e| e.to_string())?;
        return Ok(json!("ok"));
    }
    let titles: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
    // Left up for a picture with `keep: true`; put away otherwise.
    if !titles.is_empty() && cmd["keep"].as_bool() != Some(true) {
        cx.update_window(window, |_, window, cx| window.press("escape", cx)).map_err(|e| e.to_string())?;
    }
    Ok(json!(format!("ok {}", serde_json::to_string(&titles).unwrap_or_default())))
}
