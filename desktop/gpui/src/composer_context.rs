// The context ring's popover (ContextPopover and BreakdownView in
// ComposerControls.swift): the context in tokens, what happens when it fills,
// the model and login the thread is really running on and what it has cost.
// On a Claude thread it also shows what fills the context, in Claude's own
// words (`threads.contextUsage`): "64% used 642k / 1M", a stacked bar, a row
// each with its colour, tokens and share, the tool schemas kept out of it, and
// how old the count is, with a button to ask again. Asked when the popover
// opens and from that button, never on a timer. What's in it is
// threadbear_core::context_breakdown's.
use crate::style;
use gpui_kit::component::popover::PopoverState;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use threadbear_client::runner::Client;
use threadbear_core::composer::ContextMeter;
use threadbear_core::context_breakdown::{ContextAnswer, ContextBreakdown, Swatch};
use crate::tips::Tip as _;

/// The last answer for each thread, kept while the app runs, so the popover
/// opens on it while it asks again (AppModel.contextUsage); and the threads
/// being asked (contextUsageLoading).
static ANSWERS: Mutex<Option<HashMap<String, ContextAnswer>>> = Mutex::new(None);
static LOADING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn answer(thread: &str) -> Option<ContextAnswer> {
    ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|a| a.get(thread).cloned())
}

fn loading(thread: &str) -> bool {
    LOADING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|l| l.contains(thread))
}

/// Asks the thread's runner what fills its context (AppModel.loadContextUsage):
/// a Claude thread's only, one ask at a time. Another Mac's thread through
/// the backend, or an older runner, has none to give: the popover keeps to the
/// totals, and a breakdown kept from before goes, as nobody can bring it up
/// to date.
pub(crate) fn load(facts: &ContextFacts, cx: &mut App) {
    if facts.provider != "claude" || !facts.asks || facts.thread.is_empty() {
        return;
    }
    let (thread, client) = (facts.thread.clone(), facts.client.clone());
    {
        let mut asking = LOADING.lock().unwrap_or_else(|e| e.into_inner());
        if !asking.get_or_insert_with(HashSet::new).insert(thread.clone()) {
            return;
        }
    }
    cx.spawn(async move |cx| {
        let result = crate::call(client, "threads.contextUsage", json!({ "threadId": thread })).await;
        {
            let mut answers = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
            let answers = answers.get_or_insert_with(HashMap::new);
            match result {
                Ok(v) => {
                    answers.insert(thread.clone(), ContextAnswer::decode(&v));
                }
                Err(_) => {
                    answers.remove(&thread);
                }
            }
        }
        if let Some(asking) = LOADING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            asking.remove(&thread);
        }
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
    cx.refresh_windows();
}

/// What the popover shows besides the breakdown, read when the box is drawn.
pub(crate) struct ContextFacts {
    pub(crate) thread: String,
    pub(crate) provider: String,
    /// The thread's on this Mac's runner, which can ask its CLI.
    pub(crate) asks: bool,
    pub(crate) meter: ContextMeter,
    pub(crate) model: String,
    pub(crate) login: String,
    pub(crate) cost: Option<String>,
    pub(crate) client: Arc<Client>,
}

/// The popover's content.
pub(crate) fn popover(facts: &Rc<ContextFacts>, cx: &mut Context<PopoverState>) -> AnyElement {
    let claude = facts.provider == "claude";
    let answer = (claude && facts.asks).then(|| answer(&facts.thread)).flatten();
    let breakdown = answer.as_ref().and_then(|a| a.usage.as_ref()).and_then(ContextBreakdown::new);
    let secondary = style::secondary_label(cx);
    let row = |name: &'static str, value: String, cx: &App| {
        h_flex().justify_between().gap(px(12.)).text_size(px(12.)).line_height(px(style::natural_line(12.))).child(div().flex_none().text_color(style::secondary_label(cx)).child(name)).child(div().min_w_0().truncate().text_color(style::label_text(cx)).child(value))
    };
    let mut column = v_flex()
        .w(px(if claude { 290. } else { 260. }))
        // Its own 14 from the edge, as the Swift popover has: the kit's
        // popover pads its content 12 all round.
        .m(gpui_kit::px(-12.))
        .p(px(14.))
        .gap(px(6.))
        .child(h_flex().justify_between().items_center().child(div().text_size(px(13.)).line_height(px(style::natural_line(13.))).font_weight(FontWeight::SEMIBOLD).child("Context")).when(claude, |s| s.child(refresh_button(facts, answer.as_ref(), cx))));
    column = match &breakdown {
        Some(breakdown) => column.child(breakdown_view(breakdown, answer.as_ref().and_then(|a| a.note.clone()), cx)),
        None => column
            .child(Progress::new("context-fill").value(facts.meter.fraction() as f32 * 100.).h(px(4.)))
            .child(h_flex().justify_between().text_size(px(12.)).line_height(px(style::natural_line(12.))).child(facts.meter.amount()).child(div().text_color(secondary).child(format!("{}%", facts.meter.percent())))),
    };
    column
        .child(div().text_size(px(11.)).line_height(px(style::natural_line(11.))).text_color(secondary).child(format!("{} compacts it by itself when it's nearly full.", if facts.provider == "codex" { "Codex" } else { "Claude" })))
        .child(div().my(px(2.)).h(gpui_kit::px(1.)).bg(style::hairline(cx)))
        .child(row("Model", facts.model.clone(), cx))
        .child(row("Login", facts.login.clone(), cx))
        .when_some(facts.cost.clone(), |s, cost| s.child(row("Cost so far", cost, cx)))
        .into_any_element()
}

/// Asks Claude again: a spinner while it asks; off once the runner has said
/// the thread has no CLI running to ask (it answers with the kept one then).
fn refresh_button(facts: &Rc<ContextFacts>, answer: Option<&ContextAnswer>, cx: &App) -> AnyElement {
    let asleep = answer.is_some_and(|a| !a.live) || !facts.asks;
    let secondary = style::secondary_label(cx);
    if loading(&facts.thread) {
        return div().size(px(18.)).flex().items_center().justify_center().child(Spinner::new().color(secondary).with_size(px(11.))).into_any_element();
    }
    let tip = if asleep { "Claude isn't running for this thread now. It can say again after your next message." } else { "Ask Claude again what fills the context" };
    let again = facts.clone();
    div()
        .id("context-refresh")
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .when(asleep, |s| s.opacity(0.4))
        .when(!asleep, |s| s.cursor_pointer().hover(|s| s.bg(style::hover_fill(cx))).on_click(move |_, _, cx| load(&again, cx)))
        .tip(tip)
        .child(crate::icons::symbol("arrow.clockwise").with_size(px(11.)).text_color(secondary))
        .into_any_element()
}

/// BreakdownView: the headline and amount, the bar, a row each, and how old
/// the count is.
fn breakdown_view(breakdown: &ContextBreakdown, note: Option<String>, cx: &App) -> AnyElement {
    let secondary = style::secondary_label(cx);
    let mut column = v_flex()
        .gap(px(5.))
        .child(
            h_flex()
                .justify_between()
                .items_baseline()
                .child(div().text_size(px(12.)).line_height(px(style::natural_line(12.))).font_weight(FontWeight::SEMIBOLD).child(breakdown.headline()))
                .child(div().text_size(px(12.)).line_height(px(style::natural_line(12.))).text_color(secondary).child(breakdown.amount())),
        )
        .child(div().pb(px(2.)).child(bar(breakdown, cx)));
    for (n, row) in breakdown.rows.iter().enumerate() {
        let quiet = row.deferred || matches!(row.swatch, Swatch::Buffer | Swatch::Free);
        let tip = row.deferred.then_some("Tool definitions Claude loads only when it needs one, so they aren't in the context yet.");
        let line = h_flex()
            .id(("context-row", n))
            .gap(px(7.))
            .items_center()
            .text_size(px(12.))
            .line_height(px(style::natural_line(12.)))
            .child(swatch(row.swatch, row.deferred, cx))
            .child(div().flex_1().min_w_0().truncate().text_color(if quiet { secondary } else { style::label_text(cx) }).child(row.name.clone()))
            .child(div().flex_none().text_color(secondary).child(row.token_text()))
            .child(div().flex_none().w(px(38.)).flex().justify_end().text_color(secondary).child(row.percent_text().unwrap_or_default()));
        column = column.child(match tip {
            Some(tip) => line.tip(tip).into_any_element(),
            None => line.into_any_element(),
        });
    }
    if let Some(at) = breakdown.at {
        let words = format!("Breakdown from {}", threadbear_core::context_breakdown::age(at, threadbear_core::chrono::Utc::now().timestamp_millis()));
        column = column.child(
            h_flex()
                .pt(px(2.))
                .gap(px(4.))
                .items_center()
                .text_size(px(11.))
                .line_height(px(style::natural_line(11.)))
                .text_color(secondary)
                .child(words)
                .when_some(note, |s, note| s.child(div().id("context-note").child(crate::icons::symbol("exclamationmark.circle").with_size(px(11.)).text_color(secondary)).tip(note.clone()))),
        );
    }
    column.into_any_element()
}

/// What's used from the left, the buffer Claude keeps for compacting at the
/// right, the free space between: a 6-point capsule.
fn bar(breakdown: &ContextBreakdown, cx: &App) -> AnyElement {
    let pieces: Vec<(f32, Hsla, bool)> = breakdown.bar().iter().map(|r| (r.fraction.unwrap_or(0.).clamp(0., 1.) as f32, swatch_color(r.swatch, cx), r.swatch == Swatch::Buffer)).collect();
    let free = swatch_color(Swatch::Free, cx);
    canvas(|_, _, _| (), move |bounds, _, window, _| {
        let width = f32::from(bounds.size.width);
        let radius = bounds.size.height / 2.;
        // A piece of the capsule from `x`, `w` wide, its ends round where
        // they're the capsule's.
        let piece = |x: f32, w: f32, color: Hsla, window: &mut Window| {
            let (round_left, round_right) = (x <= 0.01, x + w >= width - 0.01);
            let corners = Corners { top_left: if round_left { radius } else { Pixels::ZERO }, bottom_left: if round_left { radius } else { Pixels::ZERO }, top_right: if round_right { radius } else { Pixels::ZERO }, bottom_right: if round_right { radius } else { Pixels::ZERO } };
            window.paint_quad(fill(Bounds::new(point(bounds.origin.x + gpui_kit::px(x), bounds.origin.y), size(gpui_kit::px(w), bounds.size.height)), color).corner_radii(corners));
        };
        piece(0., width, free, window);
        // A sliver for a small row, so it still shows; never wider than the bar.
        let wide = |fraction: f32| (width * fraction).max(1.5);
        let mut left = 0.;
        for (fraction, color, _) in pieces.iter().filter(|p| !p.2) {
            let w = wide(*fraction).min(width - left).max(0.);
            piece(left, w, *color, window);
            left += w;
        }
        let mut right = width;
        for (fraction, color, _) in pieces.iter().rev().filter(|p| p.2) {
            let w = wide(*fraction).min(right).max(0.);
            right -= w;
            piece(right, w, *color, window);
        }
    })
    .w_full()
    .h(px(6.))
    .into_any_element()
}

/// A row's colour: muted, so the bar reads at a glance without the popover
/// turning into a rainbow (Swatch.color).
fn swatch_color(swatch: Swatch, cx: &App) -> Hsla {
    let rgb = |r: f32, g: f32, b: f32| Hsla::from(Rgba { r, g, b, a: 1. });
    match swatch {
        Swatch::Messages => rgb(0.42, 0.58, 0.95),
        Swatch::Memory => rgb(0.40, 0.75, 0.58),
        Swatch::Skills => rgb(0.68, 0.55, 0.92),
        Swatch::Agents => rgb(0.35, 0.72, 0.80),
        Swatch::Mcp => rgb(0.86, 0.68, 0.32),
        Swatch::System => style::primary_opacity(cx, 0.42),
        Swatch::Other => style::primary_opacity(cx, 0.30),
        Swatch::Buffer => style::primary_opacity(cx, 0.22),
        Swatch::Free => style::primary_opacity(cx, 0.10),
    }
}

/// A row's colour square, 9 by 9, hollow for a deferred row.
fn swatch(swatch: Swatch, hollow: bool, cx: &App) -> impl IntoElement {
    let color = swatch_color(swatch, cx);
    div().flex_none().size(px(9.)).rounded(px(2.)).when(!hollow, |s| s.bg(color)).when(hollow, |s| s.border_1().border_color(style::secondary_label(cx)))
}

/// The popover's lines for the automation's `composer-menu` on `context`,
/// after the meter's two: the breakdown, once the runner has answered.
#[cfg(feature = "automation")]
pub(crate) fn breakdown_lines(thread: &str) -> Vec<String> {
    answer(thread).and_then(|a| a.usage).as_ref().and_then(ContextBreakdown::new).map(|b| b.lines(threadbear_core::chrono::Utc::now().timestamp_millis())).unwrap_or_default()
}
