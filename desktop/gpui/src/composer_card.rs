// The open card at the top of the composer (ComposerCard.swift, with the
// card's text and buttons from RowRenderer.asksText): Needs you with its asks,
// or Done with its follow-ups, drawn as ordinary elements. The Swift app's card
// is one attributed string with buttons drawn in it; here each ask is a block,
// which is how the transcript's rows are built too.
use crate::composer::Composer;
use crate::style;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Enter, InputEvent, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use threadbear_core::composer::{self, OWN_ANSWER};
use threadbear_core::reducer::{Ask, AskNote, AsksCard, RowContent};
use crate::tips::Tip as _;

/// What Mike has picked on the open card, kept with the composer so it
/// survives moving to another thread and back (AppModel's askPicks, nextPicks
/// and collapsedCards).
#[derive(Default)]
pub(crate) struct CardState {
    /// The card these are for.
    pub(crate) seq: Option<i64>,
    pub(crate) picks: BTreeMap<i64, i64>,
    pub(crate) notes: BTreeMap<i64, AskNote>,
    pub(crate) next_picks: BTreeSet<i64>,
    pub(crate) explain: BTreeSet<i64>,
    /// Folded to its heading (ComposerCard's chevron).
    pub(crate) collapsed: bool,
    /// The ask a paged card is showing.
    pub(crate) page: usize,
    /// The slide between two pages while it's going (CardDeck).
    slide: Option<PageSlide>,
    slide_serial: u64,
    /// The boxes under "Something else", by ask.
    inputs: HashMap<usize, Entity<TextareaState>>,
    /// Where each box is on screen, measured as it's drawn.
    box_bounds: BoxBounds,
    subs: Vec<Subscription>,
}

/// Each Something else box's place in the window, by ask, for a file let go
/// over it.
#[derive(Default)]
struct BoxBounds(std::cell::RefCell<HashMap<usize, std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>>>);

impl BoxBounds {
    fn entry_cell(&self, ask: usize) -> std::rc::Rc<std::cell::Cell<Bounds<Pixels>>> {
        self.0.borrow_mut().entry(ask).or_default().clone()
    }

    fn get(&self, ask: usize) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(&ask).map(|cell| cell.get())
    }
}

impl CardState {
    /// The Something else boxes there are, whose chips count in each.
    pub(crate) fn answer_inputs(&self) -> Vec<Entity<TextareaState>> {
        self.inputs.values().cloned().collect()
    }
}

/// A paged card going from one ask to another: the next pushes in from the
/// right as the old goes off left (back the other way), and the page number
/// rolls (Mike picked it from a mockup, 7 Nov).
#[derive(Clone, Copy)]
struct PageSlide {
    from: usize,
    to: usize,
    serial: u64,
}

/// How long the slide takes (CardDeck.duration).
const SLIDE: Duration = Duration::from_millis(180);

/// Easing out, as Core Animation's `.easeOut` does it near enough.
fn ease_out(t: f32) -> f32 {
    1. - (1. - t) * (1. - t)
}

impl Composer {
    /// The open card the thread's transcript has (TranscriptModel.openCard),
    /// as long as it's open.
    pub(crate) fn open_card(&self, cx: &App) -> Option<AsksCard> {
        let pane = self.pane.upgrade()?;
        let row = pane.read(cx).feed.model().open_card()?;
        let RowContent::Asks(card) = &row.content else { return None };
        card.is_open.then(|| card.clone())
    }

    /// The card's state, reset when a new card turns up: it opens on its
    /// first unanswered ask.
    fn sync_card(&mut self, card: &AsksCard) {
        if self.card.seq != Some(card.seq) {
            self.card = CardState { seq: Some(card.seq), ..Default::default() };
        }
    }

    /// Hands the picks to the reducer, so the rest of the app reads them off
    /// the row as the Swift app's does (setAskPicks and friends), and puts the
    /// picks back after the pane has been remade.
    pub(crate) fn push_card_state(&self, cx: &mut App) {
        let (Some(pane), Some(seq)) = (self.pane.upgrade(), self.card.seq) else { return };
        let (picks, notes, next, explain) = (self.card.picks.clone(), self.card.notes.clone(), self.card.next_picks.clone(), self.card.explain.clone());
        pane.update(cx, |p, cx| p.set_card_picks(seq, picks, notes, next, explain, cx));
    }

    /// Picks an answer, or unpicks it when it's already the pick
    /// (AppModel.pickAnswer). A picked answer on a paged card moves on to the
    /// next one still waiting, a beat later so the tick shows first.
    pub(crate) fn pick_answer(&mut self, ask: usize, option: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.open_card(cx) else { return };
        self.sync_card(&card);
        let key = ask as i64;
        let picked = self.card.picks.get(&key) == Some(&option);
        if picked {
            self.card.picks.remove(&key);
        } else {
            self.card.picks.insert(key, option);
        }
        self.push_card_state(cx);
        cx.notify();
        if !picked && option == OWN_ANSWER {
            // A box on another page of a paged card can't take the cursor
            // (only a script reaches this), so it stays in the composer.
            if !is_paged(&card) || self.card.page == ask {
                self.focus_answer_box(ask, window, cx);
            }
            return;
        }
        if !picked && option != OWN_ANSWER && is_paged(&card) {
            let page = self.card.page;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(300)).await;
                this.update_in(cx, |this, _window, cx| {
                    if this.card.page != page {
                        return;
                    }
                    if let Some(card) = this.open_card(cx) {
                        if let Some(next) = composer::next_unanswered(page, card.asks.len(), &this.card.picks, false) {
                            this.set_page(next, cx);
                        }
                    }
                })
                .ok();
            })
            .detach();
        } else if !picked {
            self.focus(window, cx);
            crate::web_panels::keyboard_to_app(window);
        }
    }

    fn set_note(&mut self, ask: usize, text: String, images: Vec<threadbear_core::reducer::ImageRef>, cx: &mut Context<Self>) {
        let note = self.card.notes.entry(ask as i64).or_default();
        note.text = text;
        note.images = images;
        self.push_card_state(cx);
        cx.notify();
    }

    /// What's in a Something else box changed (AskAnswerBox.textDidChange):
    /// its words and the pictures whose chips are in, in order, are that
    /// ask's answer.
    pub(crate) fn answer_box_changed(&mut self, input: &Entity<TextareaState>, cx: &mut Context<Self>) {
        let Some(ask) = self.card.inputs.iter().find(|(_, i)| *i == input).map(|(ask, _)| *ask) else { return };
        let text = input.read(cx).value().to_string();
        let (draft, images) = threadbear_core::inline_chips::draft_in(&text, &self.box_chips_in(input, cx), 0..text.len());
        self.set_note(ask, draft, images, cx);
    }

    fn toggle_follow_up(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let key = index as i64;
        if !self.card.next_picks.remove(&key) {
            self.card.next_picks.insert(key);
        }
        self.push_card_state(cx);
        cx.notify();
        self.focus(window, cx);
        crate::web_panels::keyboard_to_app(window);
    }

    fn toggle_explain(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let key = index as i64;
        if !self.card.explain.remove(&key) {
            self.card.explain.insert(key);
        }
        self.push_card_state(cx);
        cx.notify();
        self.focus(window, cx);
        crate::web_panels::keyboard_to_app(window);
    }

    /// ‹ and ›, and moving on after a pick: the asks slide like a deck while
    /// the heading stays put (ComposerCardHost.showPage). A click meanwhile
    /// goes to the page it's sliding to.
    fn set_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let from = self.card.page;
        self.card.page = page;
        if from != page {
            self.card.slide_serial += 1;
            let serial = self.card.slide_serial;
            self.card.slide = Some(PageSlide { from, to: page, serial });
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SLIDE + Duration::from_millis(20)).await;
                this.update(cx, |this, cx| {
                    if this.card.slide.is_some_and(|s| s.serial == serial) {
                        this.card.slide = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn toggle_collapsed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.card.collapsed = !self.card.collapsed;
        cx.notify();
        self.focus(window, cx);
        crate::web_panels::keyboard_to_app(window);
    }

    fn archive_thread(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.toggle_archive(window, cx));
        }
    }

    /// The box under an ask's "Something else" (AskAnswerBox): its own text
    /// field, kept for as long as the card is open.
    fn answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        if let Some(input) = self.card.inputs.get(&ask) {
            return input.clone();
        }
        let existing = self.card.notes.get(&(ask as i64)).cloned().unwrap_or_default();
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Say something else…").auto_grow(1, 8).submit_on_enter(true));
        // Its words are only here until they're sent: an update's restart
        // waits for them (unsent.rs).
        crate::unsent::watch(&input, cx);
        // What was answered before (a refused send put back), its pictures
        // as chips again.
        if !existing.is_empty() {
            let content = self.content_for(&existing.text, &existing.images);
            input.update(cx, |s, cx| s.set_value(content, window, cx));
        }
        let sub = cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => this.answer_box_changed(input, cx),
            InputEvent::PressEnter { secondary, shift: false } => this.answer_box_return(ask, *secondary, window, cx),
            _ => {}
        });
        self.card.subs.push(sub);
        self.card.inputs.insert(ask, input.clone());
        input
    }

    /// Pictures for an ask's box, from its picture button
    /// (AskAnswerBox.choosePictures): chips where the cursor was, as a paste
    /// would put them.
    fn choose_answer_pictures(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                this.update_in(cx, |this, window, cx| {
                    let input = this.answer_box(ask, window, cx);
                    this.take_paths_in(&input, paths, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn focus_answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.answer_box(ask, window, cx);
        input.update(cx, |s, cx| s.focus(window, cx));
        crate::web_panels::keyboard_to_app(window);
    }

    /// Return in a Something else box goes on to the next ask still
    /// unanswered, and sends only once there's none; ⌘Return sends whatever's
    /// answered (TranscriptController.answerBoxReturn).
    fn answer_box_return(&mut self, ask: usize, command: bool, window: &mut Window, cx: &mut Context<Self>) {
        if command || !self.show_unanswered(ask, true, window, cx) {
            self.send(window, cx);
        }
    }

    /// Puts the next ask with nothing picked in front: its page on a paged
    /// card, or the Swift app's beep when they're all in sight already. False
    /// when none is left (TranscriptController.showUnanswered).
    pub(crate) fn show_unanswered(&mut self, ask: usize, or_this: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(card) = self.open_card(cx) else { return false };
        let filled = composer::filled_picks(&self.card.picks, &self.card.notes);
        let Some(next) = composer::next_unanswered(ask, card.asks.len(), &filled, or_this) else { return false };
        if is_paged(&card) && self.card.page != next {
            self.set_page(next, cx);
            self.focus(window, cx);
            crate::web_panels::keyboard_to_app(window);
        } else {
            // In sight already: the beep, as there's nowhere further to go.
            crate::composer_answers::beep();
        }
        true
    }

    /// Return with nothing typed and picks on the card
    /// (ComposerView.returnWhenEmpty): the next unanswered ask, or send them.
    pub(crate) fn return_with_picks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.card.picks.is_empty() && self.card.next_picks.is_empty() && self.card.explain.is_empty() {
            return false;
        }
        if !self.card.picks.is_empty() && self.show_unanswered(self.card.page, true, window, cx) {
            return true;
        }
        self.send(window, cx);
        true
    }

    /// What the card puts in the message (AppModel.send): the answers go
    /// first, each ask then its answer, and the box's own words after them,
    /// or the ticked follow-ups. None when the card adds nothing.
    pub(crate) fn card_message(&self, words: &str, cx: &App) -> Option<(String, Vec<threadbear_core::reducer::ImageRef>, Value)> {
        let card = self.open_card(cx)?;
        if card.is_done() {
            let ticked: Vec<String> = card.next.iter().enumerate().filter(|(i, _)| self.card.next_picks.contains(&(*i as i64))).map(|(_, t)| t.clone()).collect();
            let explain: Vec<String> = card.next.iter().enumerate().filter(|(i, _)| self.card.explain.contains(&(*i as i64))).map(|(_, t)| t.clone()).collect();
            if ticked.is_empty() && explain.is_empty() {
                return None;
            }
            return Some((composer::follow_up_message(&ticked, &explain, words), Vec::new(), Value::Null));
        }
        let answers = composer::answers(&card.asks, &self.card.picks, &self.card.notes);
        if answers.is_empty() {
            return None;
        }
        let answers_json: Vec<Value> = answers.iter().map(|(_, a)| json!({ "action": a.action, "answer": a.answer })).collect();
        let images = composer::note_images(&card.asks, &self.card.picks, &self.card.notes);
        Some((composer::answers_message(&card.asks, &self.card.picks, &self.card.notes, words), images, Value::Array(answers_json)))
    }

    /// What the box's card asks for next: its placeholder, by what's picked
    /// (ComposerView.placeholder).
    pub(crate) fn card_placeholder(&self, cx: &App) -> &'static str {
        let needs_you = self.open_card(cx).is_some_and(|c| !c.is_done());
        if !self.card.picks.is_empty() || !self.card.next_picks.is_empty() || !self.card.explain.is_empty() {
            "Add anything else…"
        } else if needs_you {
            "Or say something else…"
        } else {
            "Message"
        }
    }

    /// Everything goes as one message, so the picks go too when it's sent.
    pub(crate) fn clear_picks(&mut self, cx: &mut App) {
        self.card.picks.clear();
        self.card.notes.clear();
        self.card.next_picks.clear();
        self.card.explain.clear();
        self.card.inputs.clear();
        self.card.box_bounds = BoxBounds::default();
        self.card.subs.clear();
        self.push_card_state(cx);
    }

    // ---- Drawing ----

    /// The card, over a line, at the top of the box.
    pub(crate) fn render_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.open_card(cx)?;
        self.sync_card(&card);
        let body = if card.is_done() { self.render_done_card(&card, cx) } else { self.render_asks_card(&card, window, cx) };
        // Under half the window (ComposerCardHost.maxHeight), for the card
        // itself: its padding comes on top, as the Swift box adds it outside.
        let max_height = gpui_kit::px((f32::from(window.viewport_size().height) * 0.45).max(style::zoomed(200.))) + px(18.);
        let column = div().id("composer-card").w_full().px(px(style::COMPOSER_PADDING)).pt(px(8.)).pb(px(10.)).max_h(max_height).overflow_y_scroll().child(body);
        // The separator under it, half a point as the Swift box's is.
        Some(v_flex().w_full().child(column).child(div().w_full().h(px(0.5)).bg(style::hairline(cx))).into_any_element())
    }

    /// RowRenderer.doneText: Done, with Archive at the right, and the agent's
    /// follow-ups under "Or carry on", each a row with a tick and a "?".
    fn render_done_card(&mut self, card: &AsksCard, cx: &mut Context<Self>) -> AnyElement {
        let mut column = v_flex().w_full();
        let this = cx.entity().downgrade();
        column = column.child(
            h_flex()
                .w_full()
                .h(px(metrics::PAGED_HEADING))
                .justify_between()
                .child(card_title(crate::icons::tall_checkmark().with_size(px(16.)).text_color(style::tick_green(cx)).into_any_element(), 2.5, "Done", cx))
                .child(card_button(("done-archive", 0usize), "Archive", ButtonLook::Plain, "Archive this thread", cx).on_click({
                    let this = this.clone();
                    move |_, window, cx| {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| this.archive_thread(window, cx));
                        }
                    }
                })),
        );
        // What's still open (doneText): a booked check-in alone goes under
        // "Coming back", as it doesn't stop an archive.
        if let Some(ends) = &card.loose_ends {
            if !ends.is_empty() {
                let open = ends.iter().any(|e| !e.is_booked());
                column = column.child(small_label(if open { "Still open" } else { "Coming back" }, cx));
                // Open ones first, then the check-ins (looseEndsText), each a
                // body line 4 under the last, its mark where a row's circle goes.
                for (n, end) in ends.iter().filter(|e| !e.is_booked()).chain(ends.iter().filter(|e| e.is_booked())).enumerate() {
                    let booked = end.is_booked();
                    let mark = if booked { crate::icons::symbol("clock").text_color(style::secondary_text(cx)) } else { crate::icons::symbol("exclamationmark.circle").text_color(style::needs_you_text(cx)) };
                    let words = div().flex_1().min_w_0().text_size(px(style::size::BODY)).line_height(px(style::size::BODY_LINE)).text_color(style::label_text(cx)).child(end.line(&threadbear_core::chrono::Local));
                    let mut line = h_flex()
                        .id(("loose-end", n))
                        .w_full()
                        .items_start()
                        .mt(px(8.))
                        .pl(px(metrics::ROW_INSET))
                        .child(div().flex_none().w(px(metrics::ROW_WORDS - metrics::ROW_INSET)).h(px(style::size::BODY_LINE)).flex().items_center().child(mark.with_size(px(15.))))
                        .child(words);
                    // A thread it started links to it (`thread:<id>`).
                    if let Some(id) = end.thread_id.clone() {
                        let this = this.clone();
                        line = line.cursor_pointer().tip("Open this thread").on_click(move |_, window, cx| {
                            if let Some(this) = this.upgrade() {
                                let target = format!("thread:{id}");
                                this.update(cx, |this, cx| this.open_ask_link(&target, window, cx));
                            }
                        });
                    }
                    column = column.child(line);
                }
            }
        }
        if !card.next.is_empty() {
            column = column.child(small_label("Or carry on", cx));
            for (index, follow_up) in card.next.iter().enumerate() {
                let (ticked, explained) = (self.card.next_picks.contains(&(index as i64)), self.card.explain.contains(&(index as i64)));
                let this2 = this.clone();
                let this3 = this.clone();
                // A row is a body line, 8 over and 8 under; its hover fill
                // reaches 5 past the line each way (drawHoveredRow), so the
                // fills sit 6 apart.
                column = column.child(
                    h_flex()
                        .id(("follow-up", index))
                        .w_full()
                        .items_start()
                        .mt(px(if index == 0 { 3. } else { 6. }))
                        .pl(px(metrics::ROW_INSET))
                        .py(px(5.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(style::hover_fill(cx)))
                        .tip("Send this with your next message")
                        .on_click(move |_, window, cx| {
                            if let Some(this) = this2.upgrade() {
                                this.update(cx, |this, cx| this.toggle_follow_up(index, window, cx));
                            }
                        })
                        .child(div().flex_none().w(px(metrics::ROW_WORDS - metrics::ROW_INSET)).h(px(style::size::BODY_LINE)).flex().items_center().child(tick_circle(ticked, cx)))
                        .child(div().flex_1().min_w_0().text_size(px(style::size::BODY)).line_height(px(style::size::BODY_LINE)).text_color(style::label_text(cx)).child(follow_up.clone()))
                        .child(
                            div()
                                .id(("follow-up-explain", index))
                                .flex_none()
                                .mx(px(8.))
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(style::hover_fill(cx)))
                                .child(Icon::from(IconName::CircleQuestionMark).with_size(px(14.)).text_color(if explained { style::accent(cx) } else { style::quiet_text(cx) }))
                                .tip("Ask what this means")
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(this) = this3.upgrade() {
                                        this.update(cx, |this, cx| this.toggle_explain(index, window, cx));
                                    }
                                }),
                        ),
                );
            }
        }
        column.into_any_element()
    }

    /// RowRenderer.asksText: the heading, then each ask with its answers. Over
    /// the height a card can take, one ask at a time with ‹ 1 of 3 ›, the card
    /// as tall as its tallest page so the box doesn't jump as he pages
    /// (ComposerCardHost.render).
    fn render_asks_card(&mut self, card: &AsksCard, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let collapsed = self.card.collapsed;
        let paged = !collapsed && is_paged(card);
        let page = self.card.page.min(card.asks.len().saturating_sub(1));
        let decisions = card.asks.iter().all(|a| !a.options.is_empty());
        let words = if !decisions {
            "Needed from you"
        } else if card.asks.len() == 1 {
            "Decision needed"
        } else {
            "Decisions needed"
        };
        let this = cx.entity().downgrade();
        let collapse_this = this.clone();
        // The heading line (asksHeading): a work line, or on a paged card a
        // button tall for the arrows, with the chevron at its right.
        let mut heading = h_flex()
            .w_full()
            .h(px(if paged { metrics::PAGED_HEADING } else { metrics::HEADING }))
            .items_center()
            .justify_between()
            .child(card_title(crate::icons::symbol("hand.raised.fill").with_size(px(16.5)).text_color(style::needs_you(cx)).into_any_element(), 3.5, words, cx));
        let mut right = h_flex().flex_none().items_center();
        if paged {
            let (this_back, this_next) = (this.clone(), this.clone());
            right = right
                .child(pager_arrow("‹", page > 0, "Previous ask", ("page-back", 0usize), cx, move |_, _, cx| {
                    if let Some(this) = this_back.upgrade() {
                        this.update(cx, |this, cx| this.set_page(page.saturating_sub(1), cx));
                    }
                }))
                .child(page_counter(page, card.asks.len(), self.card.slide.filter(|s| s.to == page), cx))
                .child(pager_arrow("›", page + 1 < card.asks.len(), "Next ask", ("page-next", 0usize), cx, move |_, _, cx| {
                    if let Some(this) = this_next.upgrade() {
                        this.update(cx, |this, cx| this.set_page(page + 1, cx));
                    }
                }))
                // The arrows end 7 short of the chevron (the heading's tab stop).
                .child(div().w(px(7.)));
        }
        right = right.child(
            div()
                .id("card-collapse")
                .flex_none()
                .size(px(metrics::TOGGLE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_color(style::secondary_text(cx))
                .hover(|s| s.bg(style::primary_opacity(cx, 0.08)).text_color(style::label_text(cx)))
                .active(|s| s.bg(style::primary_opacity(cx, 0.14)))
                // SF's chevron at 11 pt semibold is about 10 by 6; Lucide's,
                // a 12 by 6 stroke in 24, comes to that at 17.
                .child(Icon::from(if collapsed { IconName::ChevronUp } else { IconName::ChevronDown }).with_size(px(17.)))
                .tip(if collapsed { "Expand" } else { "Collapse" })
                .on_click(move |_, window, cx| {
                    if let Some(this) = collapse_this.upgrade() {
                        this.update(cx, |this, cx| this.toggle_collapsed(window, cx));
                    }
                }),
        );
        // On a work line the chevron, a little taller than the line, hangs
        // from its top (placeToggle).
        heading = heading.child(right.when(!paged, |r| r.self_start()));
        if collapsed {
            // Never shorter than the chevron beside it.
            return v_flex().w_full().min_h(px(metrics::TOGGLE)).child(heading).into_any_element();
        }
        let mut column = v_flex().w_full().child(heading.when(!paged, |h| h.mb(px(metrics::HEADING_SPACING))));
        let numbered = card.asks.len() > 1;
        if paged {
            // Every page in the same cell, so the cell is the tallest one's
            // height; only the page showing is drawn, and it goes last so
            // it's the one on top for the pointer.
            let slide = self.card.slide.filter(|s| s.to == page && s.from < card.asks.len());
            let mut pages = div().w_full().grid().grid_cols(1).overflow_hidden();
            for (index, ask) in card.asks.iter().enumerate().filter(|(i, _)| *i != page).chain(card.asks.iter().enumerate().filter(|(i, _)| *i == page)) {
                let body = v_flex().w_full().child(self.render_ask(index, ask, numbered, window, cx));
                let cell = div().w_full().min_w_0().col_start(1).row_start(1);
                pages = pages.child(match slide {
                    // The two asks sliding like a deck (CardDeck): the next
                    // pushes in from the right as the old goes off left, and
                    // back the other way.
                    Some(s) if index == s.from || index == s.to => {
                        let forward = s.to > s.from;
                        let (start, end) = if index == s.to { (if forward { 1. } else { -1. }, 0.) } else { (0., if forward { -1. } else { 1. }) };
                        let id = ElementId::NamedInteger("card-slide".into(), s.serial * 1000 + index as u64);
                        cell.child(body.relative().with_animation(id, Animation::new(SLIDE).with_easing(ease_out), move |el, t| el.left(relative(start + (end - start) * t))))
                    }
                    _ => cell.when(index != page, |d| d.invisible()).child(body),
                });
            }
            column = column.child(pages);
        } else {
            for (index, ask) in card.asks.iter().enumerate() {
                column = column.child(self.render_ask(index, ask, numbered, window, cx));
            }
        }
        column.into_any_element()
    }

    /// One of an ask's links: through the thread's own link handling, as a
    /// link in its transcript goes (a file into the pane, a page through the
    /// open gate, a review to its window).
    /// A path becomes a file URL the one way the app makes them, with `#`,
    /// `?`, `%` and spaces kept as part of the name: pasted into `file://`,
    /// a `#` in a folder's name cut the path short there (the open gate's
    /// source check caught it).
    fn open_ask_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = if target.starts_with('/') { threadbear_core::open_policy::file_url(target) } else { target.to_owned() };
        if let Some(pane) = self.pane.upgrade() {
            // Once this update is over: the pane reads this composer to reach
            // the app (a review's window, a file into the side pane), which
            // GPUI refuses while the composer is the one being updated.
            // "Open the design options" on a card crashed the app (12 Nov).
            window.defer(cx, move |window, cx| pane.update(cx, |p, cx| p.link_clicked(&url, "", window, cx)));
        }
    }

    /// The reading column's text style for the card's paragraphs, one per
    /// appearance and text size, as its sizes are zoomed when it's built
    /// (see `rows::reading_style`).
    fn card_reading_style(&mut self, cx: &App) -> gpui_kit::base::TextViewStyle {
        let key = (cx.theme().is_dark(), style::zoom_percent());
        match &self.card_style {
            Some((k, style)) if *k == key => style.clone(),
            _ => {
                let built = crate::rows::reading_style(cx);
                self.card_style = Some((key, built.clone()));
                built
            }
        }
    }
}

/// What drawing a card's asks (`render_ask`) needs from what it's on: the
/// composer's card (here), or a thread's decision on its project's page
/// (project_decisions.rs), so the two look and answer alike, Recommended
/// star, rows and Something else box included.
pub(crate) trait AskHost: Sized + 'static {
    /// The answer picked on this ask, `OWN_ANSWER` for Something else.
    fn ask_pick(&self, ask: usize) -> Option<i64>;
    /// Where the asks' links open from: this Mac or another, and the folder
    /// a relative file is in.
    fn ask_place(&self, cx: &App) -> (crate::open_gate::Origin, String);
    fn ask_reading_style(&mut self, cx: &App) -> gpui_kit::base::TextViewStyle;
    fn ask_pick_answer(&mut self, ask: usize, option: i64, window: &mut Window, cx: &mut Context<Self>);
    fn ask_open_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>);
    /// The card a link button shows while the pointer rests on it (a page's
    /// address, a file's picture or name), and the key its hover is kept
    /// under; None where its links have none.
    fn ask_link_card(&self, _title: &str, _target: &str, _cx: &App) -> Option<AnyElement> {
        None
    }
    fn ask_link_key(&self, _ask: usize, _link: usize) -> Option<SharedString> {
        None
    }
    fn ask_answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState>;
    /// Return in an ask's box; `command` is ⌘Return.
    fn ask_box_return(&mut self, ask: usize, command: bool, window: &mut Window, cx: &mut Context<Self>);
    /// Where an ask's command is run from (card_run.rs): its thread and that
    /// thread's runner. None shows the command with no Run.
    fn ask_run_target(&self, _cx: &App) -> Option<crate::card_run::RunTarget> {
        None
    }
    /// The box's text field, given its chips and paste where a host takes
    /// pictures in it (the composer's card does).
    fn ask_box_field(&mut self, _ask: usize, field: Textarea, _cx: &mut Context<Self>) -> Textarea {
        field
    }
    /// The pictures over the box's words and the button that adds them, where
    /// a host takes pictures; None for words alone.
    fn ask_box_pictures(&mut self, _ask: usize, _cx: &mut Context<Self>) -> Option<(Option<AnyElement>, AnyElement)> {
        None
    }
    /// Where the box is on screen, kept for a file dropped over it (the
    /// thread's drop layer is over everything while a file's dragged).
    fn ask_box_bounds(&self, _ask: usize) -> Option<std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>> {
        None
    }

    /// An ask: its paragraph (numbered on a card of several), where, its
    /// links, and its answers as buttons or rows, with Something else's box.
    fn render_ask(&mut self, index: usize, ask: &Ask, numbered: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let picked = self.ask_pick(index);
        let choices = composer::choices(ask);
        // A numbered ask's text starts past its number, as in a list in a
        // message; its buttons line up with the text, not the number.
        let indent = if numbered { px(metrics::INDENT) } else { px(0.) };
        // Its links through the open gate, from the thread's folder; with no
        // thread to say, or another runner's, no file of it opens here.
        let (origin, cwd) = self.ask_place(cx);
        // The paragraph, 12 under what's above, in a message's markdown, so a
        // list in the why looks the way it would in the message.
        let text = crate::open_gate::reading_markdown_view(("ask-text", index), ask_paragraph(ask), origin, &cwd).style(self.ask_reading_style(cx));
        let paragraph = if numbered {
            // The number where a list's goes (MarkdownRenderer.markerStart).
            h_flex()
                .w_full()
                .items_start()
                .child(div().flex_none().w(indent).pl(px(metrics::NUMBER_AT)).text_color(style::label_text(cx)).child(format!("{}.", index + 1)))
                .child(div().flex_1().min_w_0().child(text))
                .into_any_element()
        } else {
            div().w_full().child(text).into_any_element()
        };
        let mut block = v_flex().w_full().pt(px(metrics::ASK_BEFORE)).child(div().w_full().text_size(px(style::size::BODY)).line_height(px(style::size::BODY_LINE)).child(paragraph));
        // A quiet line of its own, 2 under the paragraph.
        if let Some(place) = ask.where_.as_deref().filter(|w| !w.is_empty()) {
            block = block.child(div().pl(indent).mt(px(metrics::ASK_AFTER)).text_size(px(style::size::SMALL)).line_height(px(metrics::WHERE_LINE)).text_color(style::faint_text(cx)).child(place.to_owned()));
        }
        // A command it carries, whole, with Run (card_run.rs).
        if let Some(run) = &ask.run {
            let target = self.ask_run_target(cx);
            block = block.child(crate::card_run::render_run(index, run, indent, target, cx));
        }
        let this = cx.entity().downgrade();
        let half = (metrics::BUTTON_LINE - metrics::BUTTON_HEIGHT) / 2.;
        // Its links on a line of their own, over the answers: going to look
        // comes before answering. The first is the thing to go and do.
        let home = std::env::var("HOME").unwrap_or_default();
        let links: Vec<(usize, String, String)> = ask.links.iter().enumerate().filter_map(|(n, link)| link.target(&home).map(|target| (n, link.title.clone(), target))).collect();
        let has_links = !links.is_empty();
        if has_links {
            let mut row = h_flex().w_full().pl(indent).mt(px(metrics::ASK_AFTER + metrics::BUTTONS_BEFORE)).py(px(half)).flex_wrap().gap_x(px(metrics::BUTTON_GAP)).gap_y(px(half * 2.));
            for (n, title, target) in links {
                let this = this.clone();
                let look = if n == 0 { ButtonLook::Picked } else { ButtonLook::Plain };
                // Resting on one shows where it goes, as a reply's link does
                // (RowRenderer's AskButton peek); a review has none, its
                // window being the look, and keeps its tip.
                let key = self.ask_link_key(index, n).filter(|_| crate::reply_links::has_card(&target));
                let tip: &'static str = if key.is_some() { "" } else if target.starts_with("review:") { "Open the design options" } else { "Open this" };
                let shown = key.as_ref().filter(|k| crate::chips::card_shown(k, crate::chips::SLOWER)).and_then(|_| self.ask_link_card(&title, &target, cx)).map(|card| crate::chips::under(metrics::BUTTON_HEIGHT, card));
                let button = card_button(("ask-link", index * 100 + n), &format!("{title} ↗"), look, tip, cx)
                    .relative()
                    .when_some(key, |b, key| b.on_hover(move |on, window, cx| crate::chips::hover(&key, *on, crate::chips::SLOWER, window, cx)))
                    .children(shown);
                row = row.child(button.on_click(move |_, window, cx| {
                    if let Some(this) = this.upgrade() {
                        let target = target.clone();
                        this.update(cx, |this, cx| this.ask_open_link(&target, window, cx));
                    }
                }));
            }
            block = block.child(row);
        }
        if ask.option_notes.is_empty() {
            // Buttons on 38 pt lines that wrap, 2 and then 6 under what's
            // above, four spaces apart, with Something else… after them.
            let mut buttons = h_flex()
                .w_full()
                .pl(indent)
                // 6 under the links' line, or 2 and 6 under the words.
                .mt(px(if has_links { metrics::BUTTONS_BEFORE } else { metrics::ASK_AFTER + metrics::BUTTONS_BEFORE }))
                .py(px(half))
                .flex_wrap()
                .gap_x(px(metrics::BUTTON_GAP))
                .gap_y(px(half * 2.));
            for (n, choice) in choices.iter().enumerate() {
                let is_picked = picked == Some(n as i64);
                let look = if is_picked {
                    ButtonLook::Picked
                } else if ask.suggested == Some(n) {
                    ButtonLook::Suggested
                } else {
                    ButtonLook::Plain
                };
                let title = if is_picked { format!("✓ {choice}") } else { choice.clone() };
                let this = this.clone();
                buttons = buttons.child(card_button(("answer", index * 100 + n), &title, look, "Pick this answer", cx).on_click(move |_, window, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.ask_pick_answer(index, n as i64, window, cx));
                    }
                }));
            }
            let own = picked == Some(OWN_ANSWER);
            let this2 = this.clone();
            buttons = buttons.child(card_button(("answer-own", index), if own { "✓ Something else" } else { "Something else…" }, if own { ButtonLook::Picked } else { ButtonLook::Plain }, "Answer in your own words", cx).on_click(move |_, window, cx| {
                if let Some(this) = this2.upgrade() {
                    this.update(cx, |this, cx| this.ask_pick_answer(index, OWN_ANSWER, window, cx));
                }
            }));
            block = block.child(buttons);
        } else {
            block = block.child(self.render_answer_rows(index, ask, picked, indent, cx));
        }
        if picked == Some(OWN_ANSWER) {
            let input = self.ask_answer_box(index, window, cx);
            let this3 = this.clone();
            let field = crate::composer::bare_textarea(Textarea::new(&input)).text_size(px(style::size::COMPOSER)).line_height(px(style::size::BODY_LINE));
            let field = self.ask_box_field(index, field, cx);
            // Its pictures over the words, and the button that adds them at
            // the right of its last line (AskAnswerBox).
            let (strip, add) = match self.ask_box_pictures(index, cx) {
                Some((strip, add)) => (strip, Some(add)),
                None => (None, None),
            };
            let measured = self.ask_box_bounds(index).map(|cell| canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {}).absolute().size_full());
            // AskAnswerBox: 8 under the buttons, 10 and 6 in from its edge.
            block = block.child(
                div()
                    .pl(indent)
                    .pt(px(8.))
                    .capture_action(move |action: &Enter, window, cx| {
                        if action.secondary {
                            if let Some(this) = this3.upgrade() {
                                this.update(cx, |this, cx| this.ask_box_return(index, true, window, cx));
                                cx.stop_propagation();
                            }
                        }
                    })
                    .child(
                        v_flex()
                            .relative()
                            .w_full()
                            .px(px(10.))
                            .py(px(6.))
                            .gap(px(6.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(style::hairline(cx))
                            .bg(style::answer_box_fill(cx))
                            .children(strip)
                            .child(h_flex().w_full().items_end().gap(px(4.)).child(div().flex_1().min_w_0().child(field)).children(add))
                            .children(measured),
                    ),
            );
        }
        block.into_any_element()
    }

    /// An ask's answers as rows (RowRenderer.answerRows): a circle, the label,
    /// a star and "Recommended" on the agent's, what it does under it, then "Something else…".
    fn render_answer_rows(&mut self, index: usize, ask: &Ask, picked: Option<i64>, indent: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let choices = composer::choices(ask);
        let this = cx.entity().downgrade();
        // As the Swift rows are laid out, a paragraph each (measured against
        // its composer card and decisions page, which draw them the same): a
        // box with a small corner 4 under the ask's words, 14 over the first
        // row's words (12 here, as its box starts 2 into that) and 6 over the
        // others', 2 and 6 under each (a point less where the hairline over a
        // row takes one, which the Swift rule doesn't), a 13 pt line for a
        // note just under its label, the code pad under the last, and a line
        // of 8 under the box.
        let last = choices.len();
        let mut rows = v_flex().mt(px(4.)).mb(px(8.)).ml(indent).rounded(px(3.)).overflow_hidden().bg(style::answer_rows_fill(cx)).border_1().border_color(style::primary_opacity(cx, 0.05));
        for n in 0..=choices.len() {
            let own = n == choices.len();
            let value = if own { OWN_ANSWER } else { n as i64 };
            let is_picked = picked == Some(value);
            let suggested = !own && ask.suggested == Some(n);
            let this = this.clone();
            let label = if own { "Something else…".to_owned() } else { choices[n].clone() };
            let note = (!own).then(|| ask.option_notes.get(n).cloned()).flatten().filter(|s| !s.is_empty());
            rows = rows.child(
                h_flex()
                    .id(("answer-row", index * 100 + n))
                    .w_full()
                    .items_start()
                    .gap(px(8.))
                    .px(px(10.))
                    .pt(px(if n == 0 { 12. } else { 6. }))
                    .pb(px(if n == last { 12. } else if n == 0 { 8. } else { 7. }))
                    .when(n > 0, |s| s.border_t_1().border_color(style::primary_opacity(cx, 0.055)))
                    .cursor_pointer()
                    .hover(|s| s.bg(style::hover_fill(cx)))
                    .tip(if own { "Answer in your own words" } else { "Pick this answer" })
                    .on_click(move |_, window, cx| {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| this.ask_pick_answer(index, value, window, cx));
                        }
                    })
                    .child(div().pt(px(1.)).child(tick_circle(is_picked, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex().items_baseline().gap(px(8.)).child(div().text_size(px(style::size::COMPOSER)).line_height(px(18.)).text_color(if own { style::quiet_text(cx) } else { cx.theme().foreground }).child(label)).when(suggested, |s| {
                                    // The small font, the star two points under it.
                                    s.child(
                                        h_flex()
                                            .items_baseline()
                                            .gap(px(3.))
                                            .child(div().text_size(px(9.)).text_color(style::recommended_star(cx)).child("★"))
                                            .child(div().text_size(px(style::size::SMALL)).text_color(style::secondary_label(cx)).child("Recommended")),
                                    )
                                }),
                            )
                            .when_some(note, |s, note| s.child(div().mt(px(1.)).text_size(px(style::size::SMALL)).line_height(px(13.)).text_color(style::secondary_label(cx)).child(note))),
                    ),
            );
        }
        rows.into_any_element()
    }
}

impl AskHost for Composer {
    fn ask_pick(&self, ask: usize) -> Option<i64> {
        self.card.picks.get(&(ask as i64)).copied()
    }

    /// Its links through the open gate, from the thread's folder; with no
    /// thread to say, or another runner's, no file of it opens here.
    fn ask_place(&self, cx: &App) -> (crate::open_gate::Origin, String) {
        let thread = self.chip_data(cx).summary;
        let origin = if thread.as_ref().is_some_and(|t| !t.is_remote()) { crate::open_gate::Origin::ThisMac } else { crate::open_gate::Origin::OtherMac };
        (origin, thread.map(|t| t.cwd).unwrap_or_default())
    }

    fn ask_reading_style(&mut self, cx: &App) -> gpui_kit::base::TextViewStyle {
        self.card_reading_style(cx)
    }

    fn ask_pick_answer(&mut self, ask: usize, option: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_answer(ask, option, window, cx);
    }

    fn ask_open_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_ask_link(target, window, cx);
    }

    fn ask_link_card(&self, title: &str, target: &str, cx: &App) -> Option<AnyElement> {
        self.pane.upgrade()?.read(cx).ask_link_card(title, target, cx)
    }

    fn ask_link_key(&self, ask: usize, link: usize) -> Option<SharedString> {
        Some(format!("ask-link:{}:{ask}:{link}", self.thread_id.as_deref()?).into())
    }

    fn ask_answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        self.answer_box(ask, window, cx)
    }

    fn ask_box_return(&mut self, ask: usize, command: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.answer_box_return(ask, command, window, cx);
    }

    fn ask_run_target(&self, cx: &App) -> Option<crate::card_run::RunTarget> {
        let pane = self.pane.upgrade()?;
        let pane = pane.read(cx);
        Some(crate::card_run::RunTarget { client: Some(pane.client_handle()), thread: pane.summary.id.clone(), app: self.app.clone() })
    }

    /// Pictures pasted in an ask's box go in as chips where the cursor is,
    /// as in the composer (AskAnswerBox, Mike 1 Nov).
    fn ask_box_field(&mut self, ask: usize, field: Textarea, cx: &mut Context<Self>) -> Textarea {
        let Some(input) = self.card.inputs.get(&ask).cloned() else { return field };
        let this = cx.entity().downgrade();
        let tokens = this.clone();
        let (clicks, click_input) = (this.clone(), input.clone());
        field
            .token(move |ctx, _window, cx| match tokens.upgrade() {
                Some(this) => this.read(cx).render_chip(ctx, cx),
                None => div().into_any_element(),
            })
            .on_paste(move |item, window, cx| this.upgrade().is_some_and(|this| this.update(cx, |this, cx| this.take_clipboard_in(&input, item, window, cx))))
            // A click on a picture's chip opens it among this box's others,
            // as in the message box (Composer::open_picture).
            .on_token_click(move |event, window, cx| {
                if let Some(this) = clicks.upgrade() {
                    this.update(cx, |this, cx| this.open_picture(&click_input, event.token().id(), window, cx));
                }
            })
    }

    /// Its pictures over the words, 44 square with × on hover, and the
    /// picture button at the right (AskAnswerBox's thumbs and addButton).
    fn ask_box_pictures(&mut self, ask: usize, cx: &mut Context<Self>) -> Option<(Option<AnyElement>, AnyElement)> {
        let input = self.card.inputs.get(&ask).cloned()?;
        let chips = Self::chips_in(&input, cx);
        let strip = (!chips.is_empty()).then(|| {
            let mut row = h_flex().gap(px(6.));
            for (n, (_, id)) in chips.into_iter().enumerate() {
                let path = self.pictures.get(&id).map(|c| c.image.as_ref().map_or_else(|| c.source.clone(), |i| std::path::PathBuf::from(&i.path)));
                let in_yet = self.pictures.get(&id).is_some_and(|c| c.image.is_some());
                let (remove_input, remove_id) = (input.clone(), id.clone());
                let tile = div()
                    .relative()
                    .size(px(44.))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .border(gpui_kit::px(0.5))
                    .border_color(style::hairline(cx))
                    .bg(style::chip_fill(cx))
                    .when_some(path, |d, path| d.child(img(path).size(px(44.)).object_fit(ObjectFit::Cover).when(!in_yet, |i| i.opacity(0.5))));
                let remove = crate::composer::remove_badge(("answer-picture-remove", ask * 100 + n)).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    // Not the thumbnail's click under it, which opens the picture.
                    cx.stop_propagation();
                    this.remove_picture_in(&remove_input, remove_id.clone(), window, cx);
                }));
                let (open_input, open_id) = (input.clone(), id.clone());
                let group: SharedString = format!("answer-picture-{ask}-{n}").into();
                row = row.child(
                    div()
                        .id(("answer-picture", ask * 100 + n))
                        .group(group.clone())
                        .relative()
                        .cursor_pointer()
                        .child(tile)
                        // Placed itself in the corner, as the message box's are
                        // (Composer::render_strip).
                        .child(div().absolute().top(px(2.)).right(px(2.)).invisible().group_hover(group, |s| s.visible()).child(remove))
                        .tip(crate::viewer::TILE_TIP)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.open_picture(&open_input, &open_id, window, cx);
                        })),
                );
            }
            row.into_any_element()
        });
        let add = div()
            .id(("answer-add-pictures", ask))
            .flex_none()
            .size(px(22.))
            .mb(px((style::size::BODY_LINE - 22.) / 2.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .cursor_pointer()
            .hover(|s| s.bg(style::hover_fill(cx)))
            .child(Icon::from(IconName::ImagePlus).with_size(px(14.)).text_color(style::secondary_label(cx)))
            .tip("Add pictures to this answer (or paste or drop them in the box)")
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.choose_answer_pictures(ask, window, cx)))
            .into_any_element();
        Some((strip, add))
    }

    fn ask_box_bounds(&self, ask: usize) -> Option<std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>> {
        Some(self.card.box_bounds.entry_cell(ask))
    }
}

impl Composer {
    /// Files dropped on the thread: on a Something else box they're that
    /// answer's, anywhere else the message's (ComposerDropView, AskAnswerBox).
    pub(crate) fn take_dropped_paths(&mut self, paths: Vec<std::path::PathBuf>, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        // On a paged card only the page showing has a box in sight; the
        // others keep where they were last drawn.
        let shown = self.open_card(cx).filter(|c| !self.card.collapsed && is_paged(c)).map(|_| self.card.page);
        let over = self
            .card
            .inputs
            .iter()
            .filter(|(ask, _)| shown.is_none_or(|page| page == **ask) && !self.card.collapsed)
            .find(|(ask, _)| self.card.box_bounds.get(**ask).is_some_and(|b| b.contains(&at)))
            .map(|(_, input)| input.clone());
        match over {
            Some(input) => self.take_paths_in(&input, paths, window, cx),
            None => self.take_paths(paths, window, cx),
        }
    }
}

impl Composer {
    // ---- For the automation ----

    /// The picks as the Swift dump's `decisions` says them.
    #[cfg(feature = "automation")]
    pub(crate) fn decision_lines(&self, cx: &App) -> Option<Vec<String>> {
        let card = self.open_card(cx)?;
        if card.is_done() {
            return None;
        }
        let mut lines = vec![format!("{} picked", self.card.picks.len())];
        for (i, ask) in card.asks.iter().enumerate() {
            let options = composer::choices(ask);
            // Each with what it does in brackets, when the agent said, and the
            // ask's links after, as the Swift dump's `decisions` has them (the
            // nightly parity run, scripts/parity, reads them here).
            let notes_fit = ask.option_notes.len() == ask.options.len();
            let choices: Vec<String> = options
                .iter()
                .enumerate()
                .map(|(n, o)| {
                    let star = if ask.suggested == Some(n) { "*" } else { "" };
                    let note = if notes_fit { ask.option_notes.get(n).map(|s| s.trim()).unwrap_or("") } else { "" };
                    if note.is_empty() { format!("{star}{o}") } else { format!("{star}{o} ({note})") }
                })
                .collect();
            let links = if ask.links.is_empty() { String::new() } else { format!(" links: {}", ask.links.iter().map(|l| l.title.as_str()).collect::<Vec<_>>().join(", ")) };
            let picked = self.card.picks.get(&(i as i64));
            let answer = match picked {
                // Its pictures after the words, as the Swift dump's ownAnswer.
                Some(&OWN_ANSWER) => {
                    let note = self.card.notes.get(&(i as i64)).cloned().unwrap_or_default();
                    let pictures = match note.images.len() {
                        0 => String::new(),
                        1 => " +1 picture".to_owned(),
                        n => format!(" +{n} pictures"),
                    };
                    format!("something else: \"{}\"{pictures}", note.text)
                }
                Some(n) => options.get(*n as usize).cloned().unwrap_or_else(|| "no answer".to_owned()),
                None => "no answer".to_owned(),
            };
            lines.push(format!("  {}. {} [{}]{links} -> {}", i + 1, ask.action, choices.join(" | "), answer));
        }
        Some(lines)
    }

    /// The Swift automation's `decide`: picks as its buttons pick, waiting in
    /// the composer. `pick` is "ask:choice", 1-based, choice 0 for Something else.
    #[cfg(feature = "automation")]
    pub(crate) fn decide(&mut self, cmd: &Value, window: &mut Window, cx: &mut Context<Self>) -> Result<String, String> {
        let Some(card) = self.open_card(cx).filter(|c| !c.is_done()) else { return Err("the open thread has no card of asks to answer".to_owned()) };
        self.sync_card(&card);
        if cmd["clear"].as_bool().unwrap_or(false) {
            self.card.picks.clear();
            self.push_card_state(cx);
        }
        if let Some(pick) = cmd["pick"].as_str() {
            let parts: Vec<usize> = pick.split(':').filter_map(|p| p.parse().ok()).collect();
            let [ask, choice] = parts.as_slice() else { return Err(format!("no choice {pick}")) };
            let Some(the_ask) = card.asks.get(ask.wrapping_sub(1)) else { return Err(format!("no choice {pick}")) };
            if *choice > composer::choices(the_ask).len() {
                return Err(format!("no choice {pick}"));
            }
            let option = if *choice == 0 { OWN_ANSWER } else { *choice as i64 - 1 };
            self.pick_answer(ask - 1, option, window, cx);
        }
        if let Some(value) = cmd["write"].as_str() {
            let Some((n, rest)) = value.split_once(':') else { return Err(format!("no ask in {value}")) };
            let Ok(n) = n.parse::<usize>() else { return Err(format!("no ask in {value}")) };
            if !(1..=card.asks.len()).contains(&n) {
                return Err(format!("no ask in {value}"));
            }
            let ask = n - 1;
            if self.card.picks.get(&(ask as i64)) != Some(&OWN_ANSWER) {
                self.pick_answer(ask, OWN_ANSWER, window, cx);
            }
            self.set_note(ask, rest.to_owned(), Vec::new(), cx);
            let input = self.answer_box(ask, window, cx);
            input.update(cx, |s, cx| s.set_value(rest.to_owned(), window, cx));
        }
        // `picture`: "ask:path", a picture into that ask's box, after its
        // words, as the Swift `decide` adds one (addNoteImages).
        if let Some(value) = cmd["picture"].as_str() {
            let Some((n, path)) = value.split_once(':') else { return Err(format!("no ask in {value}")) };
            let Some(ask) = n.parse::<usize>().ok().filter(|n| (1..=card.asks.len()).contains(n)).map(|n| n - 1) else { return Err(format!("no ask in {value}")) };
            if self.card.picks.get(&(ask as i64)) != Some(&OWN_ANSWER) {
                self.pick_answer(ask, OWN_ANSWER, window, cx);
            }
            let input = self.answer_box(ask, window, cx);
            let end = input.read(cx).value().len();
            input.update(cx, |s, cx| s.set_selected_range(end..end, cx));
            self.take_paths_in(&input, vec![std::path::PathBuf::from(path)], window, cx);
        }
        if cmd["send"].as_bool().unwrap_or(false) {
            self.send(window, cx);
            return Ok("ok sent".to_owned());
        }
        Ok(format!("ok {}", self.decision_lines(cx).unwrap_or_default().join("\n")))
    }

    /// The Swift automation's `ask`: a card's button by its action
    /// ("pick:0:1", "next:0", "explain:0", "page:1", "collapse", "archive").
    #[cfg(feature = "automation")]
    pub(crate) fn card_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(card) = self.open_card(cx) else { return Err("that card is closed".to_owned()) };
        self.sync_card(&card);
        let parts: Vec<&str> = action.split(':').collect();
        match parts.as_slice() {
            ["archive"] => self.archive_thread(window, cx),
            ["collapse"] => self.toggle_collapsed(window, cx),
            ["page", n] => self.set_page(n.parse().map_err(|_| "bad page")?, cx),
            ["next", n] => self.toggle_follow_up(n.parse().map_err(|_| "bad index")?, window, cx),
            ["explain", n] => self.toggle_explain(n.parse().map_err(|_| "bad index")?, window, cx),
            ["link", a, n] => {
                let (a, n): (usize, usize) = (a.parse().map_err(|_| "bad ask")?, n.parse().map_err(|_| "bad link")?);
                let home = std::env::var("HOME").unwrap_or_default();
                let target = card.asks.get(a).and_then(|ask| ask.links.get(n)).and_then(|l| l.target(&home)).ok_or("no link like that")?;
                self.open_ask_link(&target, window, cx);
            }
            ["pick", a, n] => {
                let (a, n): (usize, i64) = (a.parse().map_err(|_| "bad ask")?, n.parse().map_err(|_| "bad choice")?);
                self.pick_answer(a, n, window, cx);
            }
            // Rests on a link button: its card shows as it would after the
            // pause ("peek:0:0"), and "peek" alone puts it away.
            ["peek", a, n] => {
                let key = self.ask_link_key(a.parse().map_err(|_| "bad ask")?, n.parse().map_err(|_| "bad link")?).ok_or("no thread")?;
                crate::chips::show_card(Some(&key));
                window.refresh();
            }
            ["peek"] => {
                crate::chips::show_card(None);
                window.refresh();
            }
            ["run", a, _] => {
                let run = card.asks.get(a.parse::<usize>().map_err(|_| "bad ask")?).and_then(|ask| ask.run.clone()).ok_or("no command on that ask")?;
                crate::card_run::press_ask(self, &run, cx)?;
            }
            _ => return Err(format!("unknown card action {action}")),
        }
        Ok(())
    }

    /// The card as text lines, for the dump (composer.card): its heading and
    /// each follow-up or ask with what's picked.
    #[cfg(feature = "automation")]
    pub(crate) fn card_lines(&self, cx: &App) -> Option<Vec<String>> {
        let card = self.open_card(cx)?;
        let mut lines = Vec::new();
        // A link button's card while it shows: "peek: <title> | <detail>",
        // as its card says them, last.
        let home = std::env::var("HOME").unwrap_or_default();
        let peek = card.asks.iter().enumerate().flat_map(|(a, ask)| ask.links.iter().enumerate().map(move |(n, link)| (a, n, link))).find_map(|(a, n, link)| {
            let key = self.ask_link_key(a, n)?;
            let target = link.target(&home)?;
            (crate::chips::card_shown(&key, crate::chips::SLOWER) && crate::reply_links::has_card(&target)).then_some(())?;
            self.pane.upgrade()?.read(cx).ask_link_words(&link.title, &target)
        });
        if card.is_done() {
            lines.push("Done".to_owned());
            for (i, next) in card.next.iter().enumerate() {
                let tick = if self.card.next_picks.contains(&(i as i64)) { "[x]" } else { "[ ]" };
                lines.push(format!("{tick} {next}{}", if self.card.explain.contains(&(i as i64)) { " (?)" } else { "" }));
            }
        } else {
            lines.push(if self.card.collapsed { "Needs you (collapsed)".to_owned() } else { "Needs you".to_owned() });
            if is_paged(&card) && !self.card.collapsed {
                lines.push(format!("page {} of {}", self.card.page + 1, card.asks.len()));
            }
            for (i, ask) in card.asks.iter().enumerate() {
                let picked = self.card.picks.get(&(i as i64));
                let mark = if picked.is_some() { "[x]" } else { "[ ]" };
                lines.push(format!("{mark} {}. {}", i + 1, ask.action));
                if let Some(run) = &ask.run {
                    lines.push(format!("    $ {}", run.command));
                }
            }
        }
        if let Some((title, detail)) = peek {
            lines.push(format!("peek: {title} | {detail}"));
        }
        Some(lines)
    }
}

/// Over this, a card of several asks shows one at a time (ComposerCardHost.
/// pagedOver is 240 pt). The Swift app measures the laid-out text; this
/// estimates it from the same line heights, and the first frame can't be
/// measured any other way.
pub(crate) fn is_paged(card: &AsksCard) -> bool {
    if !card.is_open || card.asks.len() < 2 {
        return false;
    }
    // Characters on a line of the reading column at the body size.
    let width_chars = 95_f32;
    let height: f32 = card
        .asks
        .iter()
        .map(|ask| {
            let lines: f32 = ask_paragraph(ask).split('\n').map(|line| (line.chars().count() as f32 / width_chars).ceil().max(1.)).sum();
            let place = if ask.where_.as_deref().is_some_and(|w| !w.is_empty()) { metrics::ASK_AFTER + metrics::WHERE_LINE } else { 0. };
            let answers = if ask.option_notes.is_empty() { metrics::ASK_AFTER + metrics::BUTTONS_BEFORE + metrics::BUTTON_LINE } else { 38. * (ask.options.len() as f32 + 1.) };
            metrics::ASK_BEFORE + lines * style::size::BODY_LINE + place + answers
        })
        .sum();
    metrics::HEADING + metrics::HEADING_SPACING + height > 240.
}

/// An ask's paragraph, as the agent would have written it (Decisions.paragraph):
/// its action, then its why. The number a card of several puts by it is drawn
/// beside it, where a list's number goes, so words that would start a heading,
/// quote or list of their own ("- ", "2. ") are kept as words (askParagraph).
fn ask_paragraph(ask: &Ask) -> String {
    let action: String = ask.action.split('\n').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let why = ask.why.as_deref().unwrap_or("").trim();
    let body = if why.is_empty() {
        action
    } else {
        let ends = action.chars().rev().find(|c| !"\"'”’)]*_`".contains(*c)).is_some_and(|c| ".?!…:".contains(c));
        format!("{action}{} {why}", if ends { "" } else { "." })
    };
    escape_block_start(&body)
}

/// `text` with what would start a block of its own escaped: a backslash
/// before the marker's punctuation, which for a number is after its digits,
/// as markdown can't escape a digit ("2\. Deploy", not "\2. Deploy").
fn escape_block_start(text: &str) -> String {
    if !starts_a_block(text) {
        return text.to_owned();
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    format!("{}\\{}", &text[..digits], &text[digits..])
}

/// Whether markdown would read `text`'s start as a block of its own: a
/// heading, quote, list item, fence or HTML (askParagraph's pattern).
fn starts_a_block(text: &str) -> bool {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && text[hashes..].starts_with(char::is_whitespace) {
        return true;
    }
    if text.starts_with('>') || text.starts_with("```") || text.starts_with("~~~") || text.starts_with('<') {
        return true;
    }
    let mut chars = text.chars();
    if matches!(chars.next(), Some('-' | '+' | '*')) && chars.next().is_some_and(char::is_whitespace) {
        return true;
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    (1..=9).contains(&digits) && text[digits..].starts_with(['.', ')']) && text[digits + 1..].starts_with(char::is_whitespace)
}

/// The card's measures (RowRenderer.asksText and doneText, which build it as
/// TextKit lines, each a line height with spacing round it).
pub(crate) mod metrics {
    /// A paged card's heading, and the done card's: a button tall.
    pub const PAGED_HEADING: f32 = 38.;
    /// Otherwise the heading is a work line, 6 over the first ask.
    pub const HEADING: f32 = 20.;
    pub const HEADING_SPACING: f32 = 6.;
    /// The chevron (ComposerCardHost.toggleSize).
    pub const TOGGLE: f32 = 22.;
    /// Over an ask's paragraph, and under it and under its where line.
    pub const ASK_BEFORE: f32 = 12.;
    pub const ASK_AFTER: f32 = 2.;
    pub const WHERE_LINE: f32 = 19.;
    /// A line of buttons, 6 under what's over it: rows that wrap keep this apart.
    pub const BUTTON_LINE: f32 = 38.;
    pub const BUTTONS_BEFORE: f32 = 6.;
    /// A button's box (drawButton): the label font's ascender to descender
    /// and 7 over and under it.
    pub const BUTTON_HEIGHT: f32 = 29.3;
    /// Three no-break spaces at 13 pt each side of a button's title, two for
    /// the pager's, and four between buttons.
    pub const BUTTON_PAD: f32 = 10.75;
    pub const PAGER_PAD: f32 = 7.15;
    pub const BUTTON_GAP: f32 = 14.3;
    /// A list's indent, and where its number starts (MarkdownRenderer.listIndent, markerStart).
    pub const INDENT: f32 = 30.;
    pub const NUMBER_AT: f32 = 8.;
    /// A done card's row: its circle in by the hover fill's padding
    /// (rowInset), its words past the circle.
    pub const ROW_INSET: f32 = 8.;
    pub const ROW_WORDS: f32 = 34.;
}

/// A card's title: its mark, then the words in the title font (14 pt medium),
/// three quarters of a point lower than centring puts them, where TextKit's
/// rounded-down baseline offset leaves the Swift card's.
/// `gap` puts the words where the Swift title's space after its symbol does.
fn card_title(mark: AnyElement, gap: f32, words: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .items_center()
        .gap(px(gap))
        .child(mark)
        .child(div().relative().top(px(0.75)).text_size(px(style::size::CARD_TITLE)).line_height(px(17.)).font_weight(FontWeight::MEDIUM).text_color(style::label_text(cx)).child(words))
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ButtonLook {
    Plain,
    Suggested,
    Picked,
}

/// The soft shadow a card's button sits up on (drawButton).
fn button_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.25)).blur_radius(px(1.5))]
}

/// The card's button (AskButtonStyle): a rounded box with a soft shadow, the
/// picked one in the accent with white words, the agent's pick with an
/// accent edge, the rest a raised grey.
pub(crate) fn card_button(id: impl Into<ElementId>, title: &str, look: ButtonLook, tip: &'static str, cx: &App) -> Stateful<Div> {
    let (fill, text, edge) = match look {
        ButtonLook::Picked => (style::control_accent(cx), gpui_kit::white(), gpui_kit::transparent_black()),
        ButtonLook::Suggested => (style::button_fill(cx), style::label_text(cx), style::control_accent(cx).opacity(0.8)),
        ButtonLook::Plain => (style::button_fill(cx), style::label_text(cx), style::button_edge(cx)),
    };
    div()
        .id(id.into())
        .flex_none()
        .h(px(metrics::BUTTON_HEIGHT))
        // The edge is inside the box here, where the Swift one is on it.
        .px(px(metrics::BUTTON_PAD - 1.))
        .flex()
        .items_center()
        .rounded(px(7.5))
        .border_1()
        .border_color(edge)
        .bg(fill)
        .shadow(button_shadow())
        .text_color(text)
        .text_size(px(style::size::LABEL))
        .line_height(px(16.))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        // No tip where the button shows a card of its own instead.
        .when(!tip.is_empty(), |b| b.tip(tip))
        .child(title.to_owned())
}

/// "2 of 3" between the pager's arrows. While the asks slide the number
/// rolls, the old one up and out as the new comes up from below (CardDeck).
fn page_counter(page: usize, count: usize, slide: Option<PageSlide>, cx: &App) -> AnyElement {
    let secondary = style::secondary_label(cx);
    let words = move |text: String| div().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(text);
    let Some(slide) = slide else { return div().px(px(metrics::PAGER_PAD)).child(words(format!("{} of {count}", page + 1))).into_any_element() };
    let travel = 10.;
    let new = ElementId::NamedInteger("card-page-in".into(), slide.serial);
    let old = ElementId::NamedInteger("card-page-out".into(), slide.serial);
    let number = div()
        .relative()
        .overflow_hidden()
        .child(words((slide.to + 1).to_string()).relative().with_animation(new, Animation::new(SLIDE).with_easing(ease_out), move |el, t| el.top(px(travel * (1. - t))).opacity(t)))
        .child(words((slide.from + 1).to_string()).absolute().top_0().left_0().with_animation(old, Animation::new(SLIDE).with_easing(ease_out), move |el, t| el.top(px(-travel * t)).opacity(1. - t)));
    h_flex().px(px(metrics::PAGER_PAD)).child(number).child(words(format!(" of {count}"))).into_any_element()
}

/// ‹ and › on a paged card (asksHeading's arrows): boxed as a plain button
/// while there's a page that way, faint words otherwise, the same width either way.
fn pager_arrow(glyph: &'static str, enabled: bool, tip: &'static str, id: impl Into<ElementId>, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id.into())
        .flex_none()
        .h(px(metrics::BUTTON_HEIGHT))
        .flex()
        .items_center()
        .rounded(px(7.5))
        .text_size(px(style::size::LABEL))
        .line_height(px(16.))
        .tip(tip)
        .map(|s| {
            if enabled {
                s.px(px(metrics::PAGER_PAD - 1.))
                    .border_1()
                    .border_color(style::button_edge(cx))
                    .bg(style::button_fill(cx))
                    .shadow(button_shadow())
                    .text_color(style::label_text(cx))
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .on_click(on_click)
            } else {
                s.px(px(metrics::PAGER_PAD)).text_color(style::faint_text(cx))
            }
        })
        .child(glyph)
}

fn tick_circle(ticked: bool, cx: &App) -> impl IntoElement {
    let side = px(16.);
    if ticked {
        Icon::from(IconName::CircleCheck).with_size(side).text_color(style::accent(cx)).into_any_element()
    } else {
        div().size(side).rounded_full().border_1().border_color(style::quiet_text(cx)).into_any_element()
    }
}

/// "Or carry on" and the like: small, secondary, on a 19 pt line 10 under
/// what's above, in by the rows' inset.
fn small_label(text: &'static str, cx: &App) -> impl IntoElement {
    div().mt(px(10.)).pl(px(metrics::ROW_INSET)).text_size(px(style::size::SMALL)).line_height(px(metrics::WHERE_LINE)).text_color(style::secondary_label(cx)).child(text)
}

#[cfg(test)]
mod tests {
    // By name: the kit's prelude has a `test` of its own.
    use super::{ask_paragraph, starts_a_block};
    use threadbear_core::reducer::Ask;

    #[test]
    fn an_ask_that_reads_as_markdown_structure_stays_words() {
        for (text, escaped) in [("- remove the old key", "\\- remove the old key"), ("2. then this", "2\\. then this"), ("10) or this", "10\\) or this"), ("# not a heading", "\\# not a heading"), ("> quoted", "\\> quoted"), ("<b>", "\\<b>")] {
            assert!(starts_a_block(text), "{text}");
            assert_eq!(ask_paragraph(&Ask::new(text)), escaped);
        }
        for text in ["Accept the new agreement", "2026 was busy", "-1 is fine", "#hashtag", "10.5 points"] {
            assert!(!starts_a_block(text), "{text}");
        }
    }
}
