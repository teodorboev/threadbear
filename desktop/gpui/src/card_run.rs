// Run on the card (RowRenderer.appendRun, AppModel.pressRunButton): an ask
// that carries a command shows it whole, in a box, as what's shown is what
// Mike is agreeing to, says where it runs, and has Run, which runs it once on
// the thread's machine through the button RPC (docs/protocol.md, "Run on the
// card"). The runner tells the agent how it went. The asks are drawn by
// composer_card.rs's `AskHost::render_ask`, on the composer's card and on a
// project's page alike, which calls in here; what Run says is
// threadbear_core::card_run.
use crate::app::Threadbear;
use crate::composer_card::{card_button, metrics, AskHost, ButtonLook};
use crate::style::{self, px};
use crate::{call, reply_buttons};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::json;
use std::sync::Arc;
use threadbear_client::runner::Client;
use threadbear_core::card_run::{run_state, RunState};
use threadbear_core::reducer::AskRun;
use threadbear_core::reply_buttons::ReplyButton;

/// Where a Run is pressed: the thread's runner (None for the app's own, read
/// when it's pressed, as a card drawn by the app can't read it while it
/// draws), the thread, and the app to say a failure in.
pub(crate) struct RunTarget {
    pub(crate) client: Option<Arc<Client>>,
    pub(crate) thread: String,
    pub(crate) app: WeakEntity<Threadbear>,
}

/// The run's state as this app last heard of its button.
fn state(run: &AskRun) -> (RunState, Option<String>) {
    let button = run.button_id.as_deref().and_then(reply_buttons::known).map(|(_, b)| b);
    let pressing = run.button_id.as_deref().is_some_and(reply_buttons::is_pressing);
    run_state(run, button.as_ref(), pressing)
}

/// The command in its box, where it runs, a failure in red, then Run,
/// spinning while it's going. With nowhere to press it (`target` None) the
/// command shows without Run.
pub(crate) fn render_run<H: AskHost>(index: usize, run: &AskRun, indent: Pixels, target: Option<RunTarget>, cx: &mut Context<H>) -> AnyElement {
    let (state, failed) = state(run);
    let mono = cx.theme().mono_font_family.clone();
    // 8 under the words, the box 12 round the command (codePad), the command
    // whole however long, as the Swift box keeps it.
    let mut block = v_flex().w_full().pl(indent).child(
        div()
            .id(("ask-run-command", index))
            .mt(px(8.))
            .w_full()
            .p(px(12.))
            .rounded(px(7.))
            .bg(style::code_background(cx))
            .font_family(mono)
            .text_size(px(style::size::CODE))
            .line_height(px(style::size::CODE_LINE))
            .text_color(style::label_text(cx))
            .child(run.command.clone()),
    );
    let home = crate::files::home_dir();
    let mut notes: Vec<(String, Hsla)> = Vec::new();
    if let Some(cwd) = run.cwd.as_deref().filter(|c| !c.is_empty()) {
        notes.push((format!("In {}", threadbear_core::new_thread::abbreviate_home(cwd, &home)), style::faint_text(cx)));
    }
    if let Some(failed) = failed {
        notes.push((failed, style::failed_text(cx)));
    }
    for (i, (words, colour)) in notes.into_iter().enumerate() {
        block = block.child(div().mt(px(if i == 0 { 6. } else { 2. })).text_size(px(style::size::SMALL)).line_height(px(metrics::WHERE_LINE)).text_color(colour).child(words));
    }
    // An ask whose command the sync would change has no button: it can only
    // be run from a Mac on the runner itself, and its why says so.
    let (Some(button_id), Some(target)) = (run.button_id.clone(), target) else { return block.into_any_element() };
    let half = (metrics::BUTTON_LINE - metrics::BUTTON_HEIGHT) / 2.;
    let button = if state.can_press() {
        let host = cx.entity().downgrade();
        let run = run.clone();
        let target = Arc::new(target);
        card_button(("ask-run", index), state.title(), ButtonLook::Picked, "Run this command once, on the thread's machine. The agent hears how it went", cx).on_click(move |_, _, cx| {
            press(&run, &button_id, &target, host.clone(), cx);
        })
    } else if state == RunState::Running {
        // Pressed: it shows at once, and takes no more clicks until the
        // command has finished on the thread's machine.
        card_button(("ask-run", index), state.title(), ButtonLook::Plain, "Running on the thread's machine", cx).cursor_default().child(div().ml(px(6.)).child(Spinner::new().color(style::secondary_text(cx)).with_size(px(11.))))
    } else {
        card_button(("ask-run", index), state.title(), ButtonLook::Plain, "It ran, and the agent was told how it went", cx).cursor_default()
    };
    block.child(h_flex().w_full().mt(px(metrics::BUTTONS_BEFORE)).py(px(half)).child(button)).into_any_element()
}

/// Run (`pressRunButton`): once. Not through the reply buttons' own press:
/// the card is the only place this button lives, and what the runner checks a
/// press against, so there's nothing to go by but its id. `host` draws again
/// as it goes.
fn press<H: 'static>(run: &AskRun, button_id: &str, target: &RunTarget, host: WeakEntity<H>, cx: &mut App) {
    let Some(client) = target.client.clone().or_else(|| target.app.upgrade().map(|app| app.read(cx).client.clone())) else { return };
    if !state(run).0.can_press() || !reply_buttons::set_pressing(button_id, true) {
        return;
    }
    // Running… at once; after this update, as the host may be in one (the
    // automation's `ask` presses it from inside the composer's).
    let shown = host.clone();
    cx.defer(move |cx| {
        shown.update(cx, |_, cx| cx.notify()).ok();
    });
    let (thread, app, id) = (target.thread.clone(), target.app.clone(), button_id.to_owned());
    cx.spawn(async move |cx| {
        let res = call(client, "buttons.press", json!({ "threadId": thread, "buttonId": id })).await;
        match res {
            Ok(v) => {
                if let Some(button) = ReplyButton::decode(&v["button"]) {
                    reply_buttons::receive([button], &thread);
                }
            }
            Err(e) => {
                app.update(cx, |app, cx| app.show_error(e.message, cx)).ok();
            }
        }
        reply_buttons::set_pressing(&id, false);
        host.update(cx, |_, cx| cx.notify()).ok();
    })
    .detach();
}

/// The automation's `ask` with `run:<ask>:0`: what a click on Run does.
#[cfg(feature = "automation")]
pub(crate) fn press_ask<H: AskHost>(host: &mut H, run: &AskRun, cx: &mut Context<H>) -> Result<(), String> {
    let id = run.button_id.clone().ok_or("that ask's command has no Run here")?;
    let target = host.ask_run_target(cx).ok_or("there's nowhere to run it from here")?;
    let weak = cx.entity().downgrade();
    press(run, &id, &target, weak, cx);
    Ok(())
}

/// What the card's Run buttons say, one per ask with a command: "▶ Run: npm
/// test", "Running…: npm test", "✓ Ran: npm test" (the dump's
/// `composer.cardRuns`, as the Swift app's says them).
#[cfg(feature = "automation")]
pub(crate) fn composer_lines(composer: &crate::composer::Composer, cx: &App) -> Option<Vec<String>> {
    let card = composer.open_card(cx).filter(|c| !c.is_done())?;
    let lines: Vec<String> = card.asks.iter().filter_map(|a| a.run.as_ref()).filter(|r| r.button_id.is_some()).map(|r| format!("{}: {}", state(r).0.title(), r.command)).collect();
    (!lines.is_empty()).then_some(lines)
}
