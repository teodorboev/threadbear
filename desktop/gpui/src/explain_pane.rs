// Explain in the pane beside the thread (rich.explain): words Mike selected,
// what the small model said about them, his follow-ups and a box to ask
// another (ExplainPane.swift, ExplainModel.swift; docs/protocol.md,
// "Explain"). None of it goes into the thread. The session itself is
// threadbear_core::explain; the selection bar starts one (selection_bar.rs).
//
// It shows over the pane's section the way a file preview does, with Back to
// the section, and a preview opened after it takes its place.
use crate::app::Threadbear;
use crate::highlights::{palette, SidePane};
use crate::style::{self, size};
use crate::{call, icons};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{h_flex, spinner::Spinner, v_flex, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::collections::{HashMap, HashSet};
use threadbear_core::explain::{self as rules, ExplainSession};
use threadbear_core::inbox::ThreadSummary;

/// Every thread's explanation, as the app keeps them (one per thread, gone
/// on quit), and which threads' panes are showing theirs.
#[derive(Default)]
pub(crate) struct ExplainState {
    sessions: HashMap<String, ExplainSession>,
    showing: HashSet<String>,
    next: u64,
    /// The follow-up box per thread, made at its first draw (it needs the
    /// window), with what it says when Return's pressed.
    boxes: HashMap<String, (Entity<TextareaState>, Subscription)>,
}

/// The selection bar's Explain: a fresh side conversation for this thread,
/// in the pane, replacing any earlier one (`AppModel.explain`).
pub(crate) fn explain(app: &Entity<Threadbear>, thread: &ThreadSummary, selection: &str, message: Option<String>, cx: &mut App) {
    if selection.trim().is_empty() {
        return;
    }
    let side = app.read(cx).side.clone();
    let section = side.update(cx, |side, cx| {
        side.explain.next += 1;
        let session = ExplainSession::new(side.explain.next, selection, message);
        side.explain.sessions.insert(thread.id.clone(), session);
        side.explain.showing.insert(thread.id.clone());
        cx.notify();
        side.section_for(thread)
    });
    // Over the section it's in, opening that if it's closed.
    app.update(cx, |t, cx| {
        if !t.dock.read(cx).controller.is_showing(&section) {
            t.show_pane(&section, cx);
        }
    });
    fetch(&side, &thread.id, 0, cx);
}

/// Asks the runner for turn `index`'s answer (`threads.explain`).
fn fetch(side: &Entity<SidePane>, thread_id: &str, index: usize, cx: &mut App) {
    let Some(session) = side.read(cx).explain.sessions.get(thread_id).cloned() else { return };
    let client = side.read(cx).client.clone();
    let params = session.params(thread_id, index);
    let thread_id = thread_id.to_owned();
    side.update(cx, |_, cx| {
        cx.spawn(async move |this, cx| {
            let answer = call(client, "threads.explain", params).await;
            this.update(cx, |side, cx| {
                // Mike may have explained something else meanwhile.
                let Some(current) = side.explain.sessions.get_mut(&thread_id).filter(|s| s.id == session.id) else { return };
                let Some(turn) = current.turns.get_mut(index) else { return };
                match answer {
                    Ok(value) => turn.answer = Some(value["answer"].as_str().unwrap_or_default().to_owned()),
                    Err(e) if e.code == -32601 => turn.error = Some(rules::NOT_YET.to_owned()),
                    Err(e) => turn.error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    });
}

impl SidePane {
    /// Whether this thread's pane is showing its explanation.
    pub(crate) fn explaining(&self, thread_id: &str) -> bool {
        self.explain.showing.contains(thread_id) && self.explain.sessions.contains_key(thread_id)
    }

    /// A preview opened over the section takes the explanation's place.
    pub(crate) fn stop_explaining(&mut self, thread_id: &str) {
        self.explain.showing.remove(thread_id);
    }

    fn ask_follow_up(&mut self, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, _)) = self.explain.boxes.get(thread_id) else { return };
        let input = input.clone();
        let question = input.read(cx).value().to_string();
        let Some(index) = self.explain.sessions.get_mut(thread_id).and_then(|s| s.ask(&question)) else { return };
        input.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        fetch(&cx.entity(), thread_id, index, cx);
    }

    fn retry_explain(&mut self, thread_id: &str, index: usize, cx: &mut Context<Self>) {
        if self.explain.sessions.get_mut(thread_id).is_some_and(|s| s.retry(index)) {
            cx.notify();
            fetch(&cx.entity(), thread_id, index, cx);
        }
    }

    /// The bar over the explanation: Back to the section and its name, as a
    /// preview's (`render_header`).
    pub(crate) fn render_explain_header(&mut self, thread: &ThreadSummary, section: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let thread_id = thread.id.clone();
        let back_to = match section {
            crate::highlights::HIGHLIGHTS => "Highlights".to_owned(),
            crate::highlights::FILES => "Files".to_owned(),
            crate::highlights::PATCHES => "Patches".to_owned(),
            id => thread.panels().into_iter().find(|panel| panel.id == id).map_or_else(|| id.to_owned(), |panel| panel.title),
        };
        h_flex()
            .flex_none()
            .h(px(38.))
            .w_full()
            .pl(px(14.))
            .pr(px(6.))
            .items_center()
            .gap(px(6.))
            .border_b_1()
            .border_color(p.hairline)
            .child(self.header_button("explain-back", "chevron.left", format!("Back to {back_to}"), p, cx, move |this, cx| {
                this.stop_explaining(&thread_id);
                cx.notify();
            }))
            .child(div().flex_1().min_w_0().truncate().text_size(px(size::LABEL)).font_semibold().child("Explain"))
            .into_any_element()
    }

    /// The explanation (ExplainPane): the words, each answer, and the box.
    pub(crate) fn render_explain(&mut self, thread: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(session) = self.explain.sessions.get(&thread.id).cloned() else {
            return crate::files::pane_note("Select some words in the thread and pick Explain.", p).into_any_element();
        };
        let input = self.follow_up_box(&thread.id, window, cx);
        let origin = if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
        let reading = crate::rows::reading_style(cx);
        let (secondary, label) = (style::secondary_label(cx), style::label_text(cx));
        // The selected words, set off on the left like a quote.
        let quote = h_flex()
            .items_start()
            .gap(px(8.))
            .child(div().flex_none().w(px(2.)).self_stretch().rounded(px(1.)).bg(secondary.opacity(0.4)))
            .child(div().flex_1().min_w_0().text_size(px(size::LABEL)).line_height(px(style::natural_line(size::LABEL))).text_color(secondary).line_clamp(6).child(session.selection.clone()));
        let mut turns = v_flex().gap(px(14.)).child(quote);
        for (index, turn) in session.turns.iter().enumerate() {
            let mut block = v_flex().gap(px(6.));
            if let Some(question) = &turn.question {
                block = block.child(div().text_size(px(size::LABEL)).line_height(px(style::natural_line(size::LABEL))).font_weight(FontWeight::SEMIBOLD).text_color(label).child(question.clone()));
            }
            if let Some(answer) = &turn.answer {
                // The transcript's own Markdown styling, so an answer reads
                // like a reply in the thread; its links through the gate.
                block = block.child(
                    div()
                        .text_size(px(size::BODY))
                        .line_height(px(size::BODY_LINE))
                        .text_color(label)
                        .child(crate::open_gate::reading_markdown_view(("explain-answer", session.id as usize * 100 + index), answer.clone(), origin, &thread.cwd).selectable(true).style(reading.clone())),
                );
            } else if let Some(error) = &turn.error {
                let (thread_id, link) = (thread.id.clone(), style::link_text(cx));
                block = block.child(
                    h_flex()
                        .gap(px(8.))
                        .items_baseline()
                        .child(div().text_size(px(size::LABEL)).text_color(secondary).child(error.clone()))
                        .child(
                            crate::highlights::tipped(div().id(("explain-retry", index)).cursor_pointer().text_size(px(size::LABEL)).text_color(link), "Ask the model again")
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.retry_explain(&thread_id, index, cx)))
                                .child("Try again"),
                        ),
                );
            } else {
                block = block.child(h_flex().gap(px(6.)).items_center().child(Spinner::new().with_size(px(12.)).color(secondary)).child(div().text_size(px(size::LABEL)).text_color(secondary).child("Thinking")));
            }
            turns = turns.child(block);
        }
        let can_send = !session.is_busy() && !input.read(cx).value().trim().is_empty();
        let (accent, thread_id) = (style::accent(cx), thread.id.clone());
        let send = crate::highlights::tipped(div().id("explain-ask").flex_none().size(px(22.)).flex().items_center().justify_center().when(can_send, |d| d.cursor_pointer()), "Ask")
            .when(can_send, |d| d.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.ask_follow_up(&thread_id, window, cx))))
            .child(icons::symbol("arrow.up.circle.fill").size(px(18.)).text_color(if can_send { accent } else { secondary }));
        let follow_up = h_flex()
            .flex_none()
            .items_end()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .child(
                crate::highlights::tipped(div().id("explain-follow-up").flex_1().min_w_0(), "Ask more about this. It stays out of the thread.")
                    .child(crate::composer::bare_textarea(Textarea::new(&input)).text_size(px(size::LABEL)).line_height(px(style::natural_line(size::LABEL) + 2.))),
            )
            .child(send);
        v_flex()
            .size_full()
            .child(div().id("explain-scroll").flex_1().min_h_0().overflow_y_scroll().child(div().p(px(14.)).child(turns)))
            .child(div().flex_none().w_full().h(gpui_kit::px(1.)).bg(p.hairline))
            .child(follow_up)
            .into_any_element()
    }

    /// The thread's follow-up box, made the first time it's drawn.
    fn follow_up_box(&mut self, thread_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        if let Some((input, _)) = self.explain.boxes.get(thread_id) {
            return input.clone();
        }
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Ask a follow-up").auto_grow(1, 5).submit_on_enter(true));
        // A question typed and not asked: an update's restart waits for it.
        crate::unsent::watch(&input, cx);
        let id = thread_id.to_owned();
        let sub = cx.subscribe_in(&input, window, move |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => this.ask_follow_up(&id, window, cx),
            // The send button lights up as words go in.
            InputEvent::Change => cx.notify(),
            _ => {}
        });
        self.explain.boxes.insert(thread_id.to_owned(), (input.clone(), sub));
        input
    }

    /// The dump's preview line while the explanation shows: what was asked
    /// and where each answer is up to, as the Swift dump has it.
    #[cfg(feature = "automation")]
    pub(crate) fn explain_line(&self, thread_id: &str) -> Option<String> {
        self.explaining(thread_id).then(|| format!("explain {}", self.explain.sessions.get(thread_id).map_or_else(|| "(none)".to_owned(), ExplainSession::summary)))
    }
}

/// `{"cmd": "explain", "ask": "…"}` asks a follow-up as Return in the box
/// does; with nothing else, says the explanation's line.
#[cfg(feature = "automation")]
pub(crate) fn explain_command(app: &Entity<Threadbear>, cmd: &serde_json::Value, window: &mut Window, cx: &mut App) -> Result<serde_json::Value, String> {
    let side = app.read(cx).side.clone();
    let thread = side.read(cx).thread(cx).ok_or("no thread selected")?;
    if let Some(question) = cmd["ask"].as_str() {
        let question = question.to_owned();
        side.update(cx, |side, cx| {
            let input = side.follow_up_box(&thread.id, window, cx);
            input.update(cx, |s, cx| s.set_value(question, window, cx));
            side.ask_follow_up(&thread.id, window, cx);
        });
    }
    let line = side.read(cx).explain_line(&thread.id).ok_or("no explanation showing")?;
    Ok(serde_json::json!(format!("ok {line}")))
}
