// Decisions on a project's page (the projectDecisions experiment), a port of
// the Swift app's ProjectDecisionsView: a card for each of the project's
// threads (its parts' too) waiting on Mike with a card of asks, newest first,
// to answer one after another. Each card is its thread's: titled by it, a line
// on what it's doing, its asks drawn by the composer's own card code
// (composer_card.rs, `AskHost::render_ask`), and Open thread. Answering here
// sends what answering in the thread sends, `threads.send` with the answers,
// so the agent can't tell where it was answered. A card of one ask goes on one
// click; until the thread takes the answer its card says "Sending to" and
// takes no clicks, and if the runner refuses, the picks come back.
use std::sync::Arc;
use crate::app::Threadbear;
use crate::composer_card::AskHost;
use crate::open_gate::Origin;
use crate::style::{self, px};
use crate::{call, now_ms};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use threadbear_client::runner::Status;
use threadbear_core::composer::{self, OWN_ANSWER};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::projects::{self as rules, Project, Sections};
use threadbear_core::reducer::{Ask, AskNote, ImageRef};

/// An answer on its way from a project's page: the card it answers, and its
/// words for the card's "Sending to" line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecisionSend {
    pub(crate) asks_seq: u64,
    pub(crate) answers: Vec<String>,
}

/// What a refused answer gives back to its card.
type Picks = (BTreeMap<i64, i64>, BTreeMap<i64, AskNote>);

/// One thread's decision card: what's picked and written on it, as the
/// composer keeps its own card's (composer_card.rs, `CardState`).
pub(crate) struct DecisionCard {
    app: WeakEntity<Threadbear>,
    thread: String,
    /// The card these are for (`asksSeq`); a new card starts afresh.
    seq: Option<u64>,
    /// The asks as last drawn, and where their links open from. The page sets
    /// them before each draw, as the card can't read the app while the app
    /// is drawing it.
    asks: Vec<Ask>,
    place: (Origin, String),
    picks: BTreeMap<i64, i64>,
    notes: BTreeMap<i64, AskNote>,
    /// The boxes under "Something else", by ask.
    inputs: HashMap<usize, Entity<TextareaState>>,
    subs: Vec<Subscription>,
    style: Option<((bool, u32), gpui_kit::base::TextViewStyle)>,
}

impl DecisionCard {
    fn new(app: WeakEntity<Threadbear>, thread: String) -> Self {
        DecisionCard { app, thread, seq: None, asks: Vec::new(), place: (Origin::ThisMac, String::new()), picks: BTreeMap::new(), notes: BTreeMap::new(), inputs: HashMap::new(), subs: Vec::new(), style: None }
    }

    /// The thread's card as it is now. Picks made on an older card are
    /// dropped: a new card is something new to decide.
    pub(crate) fn prepare(&mut self, t: &ThreadSummary) {
        if self.seq != t.asks_seq {
            self.seq = t.asks_seq;
            self.picks.clear();
            self.notes.clear();
            self.inputs.clear();
            self.subs.clear();
        }
        self.asks = rules::decision_asks(t);
        self.place = (if t.is_remote() { Origin::OtherMac } else { Origin::ThisMac }, t.cwd.clone());
    }

    /// Every ask still open has an answer that would send something
    /// (`decisionAnswered`).
    fn answered(&self) -> bool {
        let filled = composer::filled_picks(&self.picks, &self.notes);
        self.asks.iter().enumerate().all(|(i, ask)| ask.ticked || filled.contains_key(&(i as i64)))
    }

    /// An answer's already on its way (`decisionInFlight`): the thread's
    /// summary can still say it's waiting for a moment, and a pick then would
    /// send a second answer. Only from a click, never while the app draws.
    fn in_flight(&self, cx: &App) -> bool {
        let Some(app) = self.app.upgrade() else { return true };
        let app = app.read(cx);
        app.threads.get(&self.thread).is_some_and(|t| rules::is_sending(t, app.projects.decision_sends.get(&self.thread).map(|s| s.asks_seq)))
    }

    fn set_note(&mut self, ask: usize, text: String, cx: &mut Context<Self>) {
        self.notes.entry(ask as i64).or_default().text = text;
        cx.notify();
    }

    /// Sends what's picked (`sendDecision`): the request the thread's own Send
    /// makes with an empty composer. The picks leave the card at once and come
    /// back if the runner doesn't take it.
    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(seq) = self.seq else { return };
        if self.in_flight(cx) {
            return;
        }
        let answers = composer::answers(&self.asks, &self.picks, &self.notes);
        if answers.is_empty() {
            return;
        }
        let text = composer::answers_message(&self.asks, &self.picks, &self.notes, "");
        let images = composer::note_images(&self.asks, &self.picks, &self.notes);
        let answers_json = Value::Array(answers.iter().map(|(_, a)| json!({ "action": a.action, "answer": a.answer })).collect());
        let words: Vec<String> = answers.iter().map(|(_, a)| a.answer.clone()).collect();
        let picks = (std::mem::take(&mut self.picks), std::mem::take(&mut self.notes));
        self.inputs.clear();
        self.subs.clear();
        cx.notify();
        let (thread, card) = (self.thread.clone(), cx.entity().downgrade());
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.send_decision(thread, seq, text, images, answers_json, words, card, picks, cx));
        }
    }

    /// The runner didn't take it: the picks come back, unless he's picked
    /// again since.
    pub(crate) fn restore(&mut self, seq: u64, picks: Picks, cx: &mut Context<Self>) {
        if self.seq == Some(seq) && self.picks.is_empty() && self.notes.is_empty() {
            (self.picks, self.notes) = picks;
            cx.notify();
        }
    }

    /// Its asks, drawn as the composer draws them, without the card's
    /// heading: the decision's own card is titled by its thread.
    pub(crate) fn render_asks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let asks = self.asks.clone();
        let numbered = asks.len() > 1;
        // The composer's card spaces its first ask under its heading; with no
        // heading the Swift card's asks start right under the status line, so
        // that space (less the point TextKit leaves) comes back off the top.
        let mut column = v_flex().w_full().mt(px(-(crate::composer_card::metrics::ASK_BEFORE - 1.)));
        for (index, ask) in asks.iter().enumerate() {
            column = column.child(self.render_ask(index, ask, numbered, window, cx));
        }
        column.into_any_element()
    }
}

impl AskHost for DecisionCard {
    fn ask_pick(&self, ask: usize) -> Option<i64> {
        self.picks.get(&(ask as i64)).copied()
    }

    fn ask_place(&self, _cx: &App) -> (Origin, String) {
        self.place.clone()
    }

    /// Run on an ask with a command (card_run.rs): through the app's runner,
    /// as its answers go (`send_decision`).
    fn ask_run_target(&self, _cx: &App) -> Option<crate::card_run::RunTarget> {
        Some(crate::card_run::RunTarget { client: None, thread: self.thread.clone(), app: self.app.clone() })
    }

    fn ask_reading_style(&mut self, cx: &App) -> gpui_kit::base::TextViewStyle {
        let key = (cx.theme().is_dark(), style::zoom_percent());
        match &self.style {
            Some((k, style)) if *k == key => style.clone(),
            _ => {
                let built = crate::rows::reading_style(cx);
                self.style = Some((key, built.clone()));
                built
            }
        }
    }

    /// A pick (`pickDecision`), or unpicking one. Once every open ask has an
    /// answer it goes straight away, as there's no composer to send from: one
    /// click answers a card of one. Something else opens its box instead, and
    /// Return there sends.
    fn ask_pick_answer(&mut self, ask: usize, option: i64, window: &mut Window, cx: &mut Context<Self>) {
        if ask >= self.asks.len() || self.in_flight(cx) {
            return;
        }
        let key = ask as i64;
        if self.picks.get(&key) == Some(&option) {
            self.picks.remove(&key);
        } else {
            self.picks.insert(key, option);
        }
        cx.notify();
        if self.picks.get(&key) != Some(&option) {
            return;
        }
        if option == OWN_ANSWER {
            let input = self.ask_answer_box(ask, window, cx);
            input.update(cx, |s, cx| s.focus(window, cx));
            crate::web_panels::keyboard_to_app(window);
        } else if self.answered() {
            self.send(cx);
        }
    }

    /// A design review opens in its window from the thread, where it lives;
    /// anything else opens where it opens (the browser, its own app), as the
    /// page has no pane of the thread's.
    fn ask_open_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        if target.starts_with("review:") {
            let thread = self.thread.clone();
            if let Some(app) = self.app.upgrade() {
                app.update(cx, |app, cx| app.open_thread_from_page(thread, window, cx));
            }
            return;
        }
        let url = if target.starts_with('/') { threadbear_core::open_policy::file_url(target) } else { target.to_owned() };
        let (origin, cwd) = self.place.clone();
        crate::open_gate::open_link(&url, origin, &cwd, cx);
    }

    /// The box under an ask's Something else (`AskAnswerBox`), kept while the
    /// card is open.
    fn ask_answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        if let Some(input) = self.inputs.get(&ask) {
            return input.clone();
        }
        let existing = self.notes.get(&(ask as i64)).map(|n| n.text.clone()).unwrap_or_default();
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Say something else…").auto_grow(1, 8).submit_on_enter(true));
        // Its words are only here until they're sent, what was typed before
        // too: an update's restart waits for them (unsent.rs).
        crate::unsent::watch(&input, cx);
        if !existing.is_empty() {
            input.update(cx, |s, cx| s.set_value(existing, window, cx));
        }
        let sub = cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => {
                let text = input.read(cx).value().to_string();
                this.set_note(ask, text, cx);
            }
            InputEvent::PressEnter { secondary, shift: false } => this.ask_box_return(ask, *secondary, window, cx),
            _ => {}
        });
        self.subs.push(sub);
        self.inputs.insert(ask, input.clone());
        input
    }

    /// Return in a box (`decisionBoxReturn`): sends once every ask has an
    /// answer, or with ⌘Return whatever's answered, as in the thread, where
    /// an early Return can't send a card half answered.
    fn ask_box_return(&mut self, _ask: usize, command: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if command || self.answered() {
            self.send(cx);
        }
    }
}

impl Threadbear {
    /// A project's threads (its parts' too) waiting on a card of asks, newest
    /// first, as its page lists them.
    pub(crate) fn decision_threads_for(&self, project_id: &str) -> Vec<ThreadSummary> {
        let sections = Sections::build(&self.projects.list, self.threads.values(), now_ms());
        let Some(section) = sections.section(project_id) else { return Vec::new() };
        rules::decision_threads(&sections.threads_with_parts(section)).into_iter().cloned().collect()
    }

    /// A thread's decision card, made the first time it's drawn. Cards whose
    /// threads aren't waiting any more go, so a new card starts afresh.
    pub(crate) fn decision_card(&mut self, thread: &str, cx: &mut Context<Self>) -> Entity<crate::project_decisions::DecisionCard> {
        if let Some(card) = self.projects.decision_cards.get(thread) {
            return card.clone();
        }
        let threads = &self.threads;
        self.projects.decision_cards.retain(|id, _| threads.get(id).is_some_and(rules::is_waiting));
        let app = cx.entity().downgrade();
        let card = cx.new(|_| DecisionCard::new(app, thread.to_owned()));
        self.projects.decision_cards.insert(thread.to_owned(), card.clone());
        card
    }

    /// Sends a decision's answers to its thread (`sendDecision`, by way of
    /// `post`): it needs the runner there to take it, as a message from the
    /// thread's own box does.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn send_decision(&mut self, thread: String, seq: u64, text: String, images: Vec<ImageRef>, answers: Value, words: Vec<String>, card: WeakEntity<DecisionCard>, picks: Picks, cx: &mut Context<Self>) {
        // As a message from the thread's own box: one of this Mac's threads
        // waits for a restarting runner (`sendDecision`'s `holding`).
        let remote = self.threads.get(&thread).is_some_and(|t| t.runner_id.is_some());
        let holding = !remote && self.client.restart_under_way();
        if !matches!(self.status, Status::Connected { .. }) && !holding {
            self.show_error("Not connected to the runner, so that wasn't sent.".to_owned(), cx);
            // Once this update is over: the card's own update is what sent
            // it (a pick, Return in its box), and GPUI refuses to update the
            // card again from inside that. Answering while the runner was
            // away crashed the app (12 Nov).
            cx.defer(move |cx| {
                card.update(cx, |card, cx| card.restore(seq, picks, cx)).ok();
            });
            return;
        }
        self.projects.decision_sends.insert(thread.clone(), DecisionSend { asks_seq: seq, answers: words });
        cx.notify();
        let mut params = json!({ "threadId": thread, "text": text });
        if !images.is_empty() {
            params["images"] = json!(images.iter().map(crate::composer::image_ref_json).collect::<Vec<_>>());
        }
        if !answers.is_null() {
            params["answers"] = answers;
        }
        let client = self.client.clone();
        // Its place in the order of this runner's requests, from the click.
        let place = (!remote).then(|| client.place(&thread));
        cx.spawn(async move |this, cx| {
            // The card says "Sending to" with its spinner meanwhile, held or not.
            let res = if let Some(place) = place {
                crate::RT.spawn(async move { client.call_in_restart_order_at(place, "threads.send", params, Arc::new(|_| {})).await }).await.unwrap_or_else(|e| Err(threadbear_client::runner::RpcError { code: -1, message: e.to_string(), data: None }))
            } else {
                call(client, "threads.send", params).await
            };
            this.update(cx, |this, cx| {
                if let Err(e) = res {
                    if this.projects.decision_sends.get(&thread).is_some_and(|s| s.asks_seq == seq) {
                        this.projects.decision_sends.remove(&thread);
                    }
                    card.update(cx, |card, cx| card.restore(seq, picks, cx)).ok();
                    this.show_error(e.message, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `DecisionsSection`: at the top of a project's page, with the
    /// projectDecisions experiment on, its threads' open cards of asks.
    pub(crate) fn render_decisions_section(&mut self, project: &Project, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.project_decisions_on() {
            return None;
        }
        let threads = self.decision_threads_for(&project.id);
        if threads.is_empty() {
            return None;
        }
        let refs: Vec<&ThreadSummary> = threads.iter().collect();
        let detail = rules::decision_heading(rules::decision_count(&refs), threads.len());
        let mut section = v_flex().w_full().gap(px(12.)).child(crate::project_page::page_heading("Decisions", Some(detail), None, cx));
        for t in &threads {
            section = section.child(self.render_decision_card(t, window, cx));
        }
        Some(section.into_any_element())
    }

    /// `DecisionCard`: the thread's title and how long ago, a line on what
    /// it's doing, its asks, and Open thread. While an answer's on its way,
    /// one line, "Sending to <title>…" with a spinner and what was answered.
    fn render_decision_card(&mut self, t: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let card = self.decision_card(&t.id, cx);
        card.update(cx, |card, _| card.prepare(t));
        let sent = self.projects.decision_sends.get(&t.id).cloned();
        let sending = rules::is_sending(t, sent.as_ref().map(|s| s.asks_seq));
        let title = if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() };
        let (faint, secondary) = (style::faint_text(cx), style::secondary_text(cx));
        let id = t.id.clone();
        let open = crate::project_page::small_push_button(SharedString::from(format!("decision-open-{}", t.id)), "Open thread", format!("Open {title} for the whole thread"), cx, cx.listener(move |this, _: &ClickEvent, window, cx| this.open_thread_from_page(id.clone(), window, cx)));
        let body = match sent.filter(|_| sending) {
            Some(sent) => h_flex()
                .w_full()
                .items_center()
                .gap(px(8.))
                .text_size(px(style::size::LABEL))
                .line_height(px(16.))
                .child(div().flex_none().size(px(12.)).child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(secondary).with_size(px(12.))))
                .child(h_flex().flex_shrink(1.).min_w_0().text_color(secondary).child(div().flex_none().child("Sending to ")).child(div().min_w_0().truncate().font_semibold().child(title.clone())).child(div().flex_none().child("…")))
                .child(div().flex_shrink(1.).min_w_0().truncate().text_color(faint).child(sent.answers.join("; ")))
                .child(div().flex_1())
                .child(open)
                .into_any_element(),
            None => {
                let when = threadbear_core::sidebar::relative_short(t.activity_ms(), &threadbear_core::sidebar::local_now());
                let asks = card.update(cx, |card, cx| card.render_asks(window, cx));
                v_flex()
                    .w_full()
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_shrink(1.).min_w_0().truncate().text_size(px(style::size::LABEL)).line_height(px(16.)).font_semibold().child(title.clone()))
                            .child(div().flex_none().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(faint).child(when))
                            .child(div().flex_1())
                            .child(open),
                    )
                    .when_some(rules::status_line(t), |s, line| {
                        s.child(crate::highlights::tipped(div().id(SharedString::from(format!("decision-line-{}", t.id))).truncate().text_size(px(style::size::DETAIL)).line_height(px(15.)).text_color(secondary), line.clone()).child(line))
                    })
                    .child(div().w_full().pt(px(2.)).child(asks))
                    .into_any_element()
            }
        };
        // Its own id, so the asks' controls in it are this card's to the
        // window, not the next card's of the same shape.
        div()
            .id(SharedString::from(format!("decision-{}", t.id)))
            .w_full()
            .px(px(16.))
            .py(px(if sending { 10. } else { 14. }))
            .rounded(px(10.))
            .bg(style::card_background(cx))
            .border(gpui_kit::px(0.5))
            .border_color(style::card_border(cx))
            .child(body)
            .into_any_element()
    }

    /// The open page's decisions in the dump (`projectDecisions`): one line a
    /// thread, "Title: first ask | second ask", "(sending)" at the end while
    /// an answer's on its way. None with the experiment off or no page open.
    #[cfg(feature = "automation")]
    pub(crate) fn project_decisions_dump(&self) -> Option<Vec<String>> {
        if !self.project_decisions_on() {
            return None;
        }
        let project = self.open_project()?;
        Some(
            self.decision_threads_for(&project.id)
                .iter()
                .map(|t| rules::decision_line(t, rules::is_sending(t, self.projects.decision_sends.get(&t.id).map(|s| s.asks_seq))))
                .collect(),
        )
    }
}

/// `decision`: a decision on the open project's page, through the card's own
/// calls, as the Swift command: `thread` (its id or title), `pick`
/// "ask:choice" (both from 1, 0 for Something else), `write` "ask:words" in
/// its Something else box, `send` as ⌘Return there. Says the page's
/// decisions after, as the dump does.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let thread = {
        let t = app.read(cx);
        if !t.project_decisions_on() {
            return Err("the projectDecisions experiment is off".to_owned());
        }
        let project = t.open_project().ok_or("no project page open")?.id.clone();
        let shown = t.decision_threads_for(&project);
        let titles = shown.iter().map(|s| s.title.clone()).collect::<Vec<_>>().join(", ");
        let key = cmd["thread"].as_str().ok_or_else(|| format!("give a thread: {titles}"))?;
        let thread = shown.iter().find(|s| s.id == key || s.title == key).cloned().ok_or_else(|| format!("no decision from {key}; there are {titles}"))?;
        thread
    };
    let card = app.update(cx, |t, cx| t.decision_card(&thread.id, cx));
    card.update(cx, |card, _| card.prepare(&thread));
    let asks = rules::decision_asks(&thread);
    if let Some(pick) = cmd["pick"].as_str() {
        let parts: Vec<usize> = pick.split(':').filter_map(|p| p.parse().ok()).collect();
        let [ask, choice] = parts.as_slice() else { return Err(format!("no choice {pick}")) };
        let Some(the_ask) = asks.get(ask.wrapping_sub(1)) else { return Err(format!("no choice {pick}")) };
        if *choice > composer::choices(the_ask).len() {
            return Err(format!("no choice {pick}"));
        }
        let option = if *choice == 0 { OWN_ANSWER } else { *choice as i64 - 1 };
        let ask = ask - 1;
        card.update(cx, |card, cx| card.ask_pick_answer(ask, option, window, cx));
    }
    if let Some(value) = cmd["write"].as_str() {
        let Some((n, words)) = value.split_once(':') else { return Err(format!("no ask in {value}")) };
        let ask = n.parse::<usize>().ok().filter(|n| (1..=asks.len()).contains(n)).ok_or_else(|| format!("no ask in {value}"))? - 1;
        let words = words.to_owned();
        card.update(cx, |card, cx| {
            if card.ask_pick(ask) != Some(OWN_ANSWER) {
                card.ask_pick_answer(ask, OWN_ANSWER, window, cx);
            }
            card.set_note(ask, words.clone(), cx);
            let input = card.ask_answer_box(ask, window, cx);
            input.update(cx, |s, cx| s.set_value(words, window, cx));
        });
    }
    if cmd["send"].as_bool() == Some(true) {
        card.update(cx, |card, cx| card.ask_box_return(0, true, window, cx));
    }
    let lines = app.read(cx).project_decisions_dump().unwrap_or_default();
    Ok(json!(format!("ok {}", lines.join("\n"))))
}
