// The column down the right (ReviewWindow.swift's `rail` and `footer`): the
// agent's lead, each option with its reasons, its shots and a tick, and under
// them one reply box with None and Send; once it's over, what was sent.
use super::ReviewWindow;
use crate::style::{self, px};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::ops::Range;
use threadbear_core::open_policy::Origin;
use threadbear_core::reducer::ReviewOption;
use threadbear_core::review::{self as rules, ReviewMode};

/// The rail's width: the Swift window's, not zoomed, as its frame isn't.
pub(super) const RAIL_WIDTH: f32 = 340.;
const PAD: f32 = 14.;
/// The rail's small pictures of an option's shots.
const RAIL_THUMB: (f32, f32) = (52., 33.);

impl ReviewWindow {
    pub(super) fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let hairline = style::hairline(cx);
        let mut list = div().id("review-rail").flex_1().min_h_0().w_full().overflow_y_scroll().track_scroll(&self.rail_scroll);
        // The agent's lead, as markdown in the text colour: drawn raw in grey it
        // was one long wall where backticks showed and nothing stood out (Mike, 7 Nov).
        if let Some(note) = self.session.review().note.clone().filter(|n| !n.is_empty()) {
            list = list.child(
                div()
                    .w_full()
                    .px(px(PAD))
                    .py(px(PAD - 1.))
                    .border_b_1()
                    .border_color(hairline)
                    .text_size(px(style::size::WORK))
                    .line_height(px(style::natural_line(style::size::WORK) + 2.))
                    .child(inline_note("review-note", &note, style::label_text(cx), &self.cwd, cx)),
            );
        }
        let options = self.session.review().options.clone();
        for (index, option) in options.iter().enumerate() {
            list = list.child(self.render_choice(index, option, cx));
        }
        v_flex()
            .id("review-rail-column")
            .flex_none()
            .w(gpui_kit::px(RAIL_WIDTH))
            .h_full()
            .overflow_hidden()
            .bg(style::primary_opacity(cx, 0.025))
            .child(list)
            .child(div().flex_none().w_full().h(gpui_kit::px(1.)).bg(hairline))
            .child(div().flex_none().w_full().p(px(PAD)).child(self.render_footer(window, cx)))
            .into_any_element()
    }

    /// One option: its tick, its name and reasons (a click shows it on the
    /// stage), its shots when it has several, and a note left on it.
    fn render_choice(&mut self, index: usize, option: &ReviewOption, cx: &mut Context<Self>) -> AnyElement {
        let picked = self.session.draft().picked_ids.contains(&option.id);
        let current = self.session.mode == ReviewMode::Single && self.session.focused() == index;
        let note = self.session.shown_draft().notes.get(&option.id).cloned().unwrap_or_default();
        let shots = option.all_shots();
        let bg = if current { style::primary_opacity(cx, 0.06) } else if picked { style::control_accent(cx).opacity(0.07) } else { gpui_kit::transparent_black() };
        let tip = if current { format!("Showing {}", option.id) } else { format!("Show {}", option.id) };
        let words = crate::highlights::tipped(div().id(("review-choice", index)).w_full().cursor_pointer(), tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !current {
                    this.enlarge(index, 0, cx);
                }
            }))
            .child(self.render_choice_text(index, option, cx));
        let mut column = v_flex().flex_1().min_w_0().gap(px(8.)).child(words);
        if shots.len() > 1 {
            let (primary, focused_shot) = (style::label_text(cx), self.session.shot());
            let mut flow = h_flex().flex_wrap().gap_x(px(5.)).gap_y(px(6.));
            for (shot, item) in shots.iter().enumerate() {
                let on = current && focused_shot == shot;
                flow = flow.child(
                    crate::highlights::tipped(div().id(SharedString::from(format!("review-rail-shot-{}-{shot}", option.id))).relative().flex_none().cursor_pointer(), rules::shot_tip(option, shot))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.enlarge(index, shot, cx)))
                        .child(super::stage::thumb(&item.image, RAIL_THUMB.0, RAIL_THUMB.1, 4., cx).opacity(if on { 1. } else { 0.7 }))
                        .when(on, |s| s.child(div().absolute().top(px(-2.)).left(px(-2.)).w(px(RAIL_THUMB.0 + 4.)).h(px(RAIL_THUMB.1 + 4.)).rounded(px(4.)).border(px(1.5)).border_color(primary))),
                );
            }
            column = column.child(flow);
        }
        // A note on one option is from before the one reply box, or a client
        // that still sends them: shown, as it'll go with the reply.
        if !note.is_empty() {
            column = column.child(div().text_size(px(style::size::WORK)).line_height(px(style::natural_line(style::size::WORK))).child(note));
        }
        h_flex()
            .relative()
            .w_full()
            .items_start()
            .gap(px(10.))
            .px(px(PAD))
            .py(px(12.))
            .border_b_1()
            .border_color(style::hairline(cx))
            .bg(bg)
            .child(self.render_pick_mark(option, picked, cx))
            .child(column)
            .when(current, |s| s.child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).bg(style::primary_opacity(cx, 0.5))))
            .into_any_element()
    }

    /// The option's name, with the agent's pick marked as on an asks card,
    /// and its reason as inline markdown.
    fn render_choice_text(&mut self, index: usize, option: &ReviewOption, cx: &mut Context<Self>) -> AnyElement {
        let recommended = self.session.review().recommended.as_deref() == Some(option.id.as_str());
        let title = h_flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(8.))
            .child(div().text_size(px(style::size::WORK)).line_height(px(style::natural_line(style::size::WORK))).font_weight(FontWeight::SEMIBOLD).child(option.label()))
            .when(recommended, |s| s.child(recommended_mark(cx)));
        let mut words = v_flex().gap(px(3.)).child(title);
        if let Some(reason) = option.note.clone().filter(|n| !n.is_empty()) {
            // SwiftUI's 1.5 between lines is a taller line here, less the
            // half of it it puts above the first line and under the last.
            words = words.child(
                div()
                    .mt(px(-0.75))
                    .mb(px(-0.75))
                    .text_size(px(style::size::DETAIL))
                    .line_height(px(style::natural_line(style::size::DETAIL) + 1.5))
                    .child(inline_note(("review-reason", index), &reason, style::secondary_text(cx), &self.cwd, cx)),
            );
        }
        words.into_any_element()
    }

    /// A tick, as several can be picked. A reference has none, nor has a sent
    /// review's unpicked option; the gap keeps the words lined up.
    fn render_pick_mark(&mut self, option: &ReviewOption, picked: bool, cx: &mut Context<Self>) -> AnyElement {
        let accent = style::control_accent(cx);
        let slot = div().flex_none().size(px(16.)).flex().items_center().justify_center();
        if self.session.editable() && !option.reference {
            let id = option.id.clone();
            let tip = if picked { format!("Unpick {}", option.id) } else { format!("Pick {}", option.id) };
            return crate::highlights::tipped(slot.id(SharedString::from(format!("review-pick-{}", option.id))).cursor_pointer(), tip)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick(&id, cx)))
                .child(checkbox(picked, accent, cx))
                .into_any_element();
        }
        if picked {
            return slot.child(checkbox(true, accent, cx)).into_any_element();
        }
        slot.into_any_element()
    }

    // The answer.

    fn render_footer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.session.review().is_pending() {
            return self.render_sent(cx);
        }
        let none = self.session.draft().none;
        let editable = self.session.editable();
        let can_send = self.session.can_submit();
        let submitting = self.session.submitting();
        let active = window.is_window_active();
        let focused = self.reply.read(cx).focus_handle(cx).is_focused(window);
        let accent = style::control_accent(cx);
        let dark = cx.theme().is_dark();
        // Grows with what's typed, from 60 to the 13 lines its box takes; a
        // click anywhere in it puts the cursor in it. The edge is drawn inside
        // its 6 of padding, as the Swift box's strokeBorder is.
        let reply = self.reply.clone();
        let reply_box = div()
            .id("review-reply")
            .w_full()
            .min_h(px(72.))
            .p(px(5.))
            .rounded(px(8.))
            .bg(hsla(0., 0., 0., if dark { 0.2 } else { 0.03 }))
            .border_1()
            .border_color(if focused { accent } else { style::hairline(cx) })
            .cursor_text()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let handle = reply.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                crate::web_panels::keyboard_to_app(window);
            })
            .child(crate::composer::bare_textarea(Textarea::new(&self.reply)).w_full().text_size(px(style::size::WORK)).line_height(px(style::natural_line(style::size::WORK))));
        // None: tinted while it's chosen, as the Swift button's tint is.
        let none_tip = if none { "Take back none of them" } else { "None of them will do" };
        let none_fill = if none && active { accent.opacity(0.25) } else { style::primary_opacity(cx, 0.08) };
        let none_button = crate::highlights::tipped(
            h_flex()
                .id("review-none")
                .flex_none()
                .w(px(98.))
                .h(px(28.))
                .rounded_full()
                .bg(none_fill)
                .justify_center()
                .items_center()
                .text_size(px(style::size::LABEL))
                .text_color(if none && active { accent } else { style::label_text(cx) })
                .when(editable, |s| s.cursor_pointer())
                .when(!editable, |s| s.opacity(0.5)),
            none_tip,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_none(cx)))
        .child(div().pt(px(1.)).child("None"));
        // Send says what will go, and while it goes says so with a spinner and
        // takes no more presses. Grey in a window that isn't in front, as a
        // Mac's own default button is.
        let (send_fill, send_text) = if can_send && active { (accent, gpui_kit::white()) } else if can_send || submitting { (style::primary_opacity(cx, 0.08), style::label_text(cx)) } else { (style::primary_opacity(cx, 0.04), style::tertiary_label(cx)) };
        let send = crate::highlights::tipped(
            h_flex()
                .id("review-send")
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .rounded_full()
                .bg(send_fill)
                .justify_center()
                .items_center()
                .gap(px(6.))
                .text_size(px(style::size::LABEL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(send_text)
                .when(can_send, |s| s.cursor_pointer())
                .when(submitting, |s| s.child(Spinner::new().color(send_text).with_size(px(12.)))),
            "Send your picks and reply to the agent (\u{2318}\u{21A9}). Close the window to finish it later.",
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.submit(cx)))
        .child(div().pt(px(1.)).min_w_0().truncate().child(self.session.send_label()));
        v_flex().w_full().gap(px(10.)).child(reply_box).child(h_flex().w_full().gap(px(8.)).child(none_button).child(send)).into_any_element()
    }

    /// What went, once it's over: None of them, the reply as written, and
    /// whether it was sent, cancelled or answered in the thread, with Done.
    fn render_sent(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let sent = self.session.shown_draft().clone();
        let status = rules::status_text(self.session.review());
        let mut column = v_flex().w_full().gap(px(10.)).text_size(px(style::size::WORK)).line_height(px(style::natural_line(style::size::WORK)));
        if sent.none {
            column = column.child(div().font_weight(FontWeight::SEMIBOLD).child("None of them"));
        }
        if let Some(reply) = sent.reply.filter(|r| !r.is_empty()) {
            // As tall as it needs up to the reply box's most, then it scrolls,
            // so a long one leaves room for the options and keeps Done on screen.
            column = column.child(div().id("review-sent-reply").w_full().max_h(px(220.)).overflow_y_scroll().child(reply));
        }
        column
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::DETAIL)).text_color(style::secondary_text(cx)).child(status))
                    .child(super::plain_button("review-done", "Done", "Close it", false, cx).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close(cx)))),
            )
            .into_any_element()
    }
}

/// An agent's note as SwiftUI draws inline markdown: bold, italic, code in
/// the mono face and links in the link colour, on the lines of what it's
/// in, so it takes the rail's line heights as the Swift rail's text does.
/// (The kit's markdown view keeps line heights of its own and boxes inline
/// code.) A link opens through the open gate.
fn inline_note(id: impl Into<ElementId>, source: &str, color: Hsla, cwd: &str, cx: &App) -> AnyElement {
    let (base, mono, link_color) = (cx.theme().font_family.clone(), cx.theme().mono_font_family.clone(), style::link_text(cx));
    let mut text = String::new();
    let mut runs: Vec<TextRun> = vec![];
    let mut links: Vec<(Range<usize>, std::sync::Arc<str>)> = vec![];
    for run in rules::inline_markdown(source) {
        let start = text.len();
        text.push_str(&run.text);
        let mut face = font(if run.code { mono.clone() } else { base.clone() });
        if run.bold {
            face.weight = FontWeight::BOLD;
        }
        if run.italic {
            face.style = FontStyle::Italic;
        }
        runs.push(TextRun { len: run.text.len(), font: face, color: if run.link.is_some() { link_color } else { color }, background_color: None, underline: None, strikethrough: None });
        if let Some(url) = run.link {
            links.push((start..text.len(), url));
        }
    }
    let styled = StyledText::new(text).with_runs(runs);
    if links.is_empty() {
        return styled.into_any_element();
    }
    // The addresses as the parser shares them, one copy a link.
    let (ranges, urls): (Vec<Range<usize>>, Vec<std::sync::Arc<str>>) = links.into_iter().unzip();
    let cwd = cwd.to_owned();
    InteractiveText::new(id, styled).on_click(ranges, move |ix, _, cx| crate::open_gate::open_link(&urls[ix], Origin::ThisMac, &cwd, cx)).into_any_element()
}

/// A tick box: an outline, or filled with the accent and a white tick.
fn checkbox(on: bool, accent: Hsla, cx: &App) -> AnyElement {
    if on {
        return div().size(px(14.)).rounded(px(3.5)).bg(accent).flex().items_center().justify_center().child(Icon::new(IconName::Check).with_size(px(11.)).text_color(gpui_kit::white())).into_any_element();
    }
    div().size(px(14.)).rounded(px(3.5)).border(px(1.2)).border_color(style::secondary_text(cx)).into_any_element()
}

/// The agent's pick: a small gold star and "Recommended" in grey, as on an
/// asks card (after Ghostex's, Mike, 9 Nov).
pub(super) fn recommended_mark(cx: &App) -> AnyElement {
    // On the name's line, as the Swift mark is text run on from the name.
    h_flex()
        .flex_none()
        .gap(px(3.))
        .items_center()
        .child(crate::icons::symbol("star.fill").with_size(px(11.)).text_color(style::recommended_star(cx)))
        .child(div().text_size(px(style::size::DETAIL)).line_height(px(style::natural_line(style::size::WORK))).text_color(style::secondary_text(cx)).child("Recommended"))
        .into_any_element()
}
