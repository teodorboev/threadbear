// The Librarian view, a port of the Swift app's LibrarianView and
// LibrarianHome: what the runner's background job that keeps projects up to
// date is doing, what it will look at next and what it has done, in the
// sidebar; its page beside it with the numbers and its two settings.
//
// Pausing shows at once and goes back if the runner says no; Look at
// everything spins until the runner has queued the looks; each takes no
// second click while it's on its way.
use crate::app::Threadbear;
use crate::home_page::{home_status_line, home_title, mode_header, Art};
use crate::sidebar::{icon, palette, tipped, Palette, LEADING, OUTER_INSET, TRAILING};
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashSet;
use threadbear_core::chrono::Local;
use threadbear_core::librarian::{self as rules, Entry, Job, Outcome, State, Status};
use crate::tips::Tip as _;

/// The Librarian's state in the app (`Threadbear::librarian`).
pub(crate) struct LibrarianState {
    /// Once `librarian.status` has answered; replaced by `librarian.updated`.
    pub(crate) status: Option<Status>,
    /// "pause" and "run" while they're on their way.
    pub(crate) busy: HashSet<&'static str>,
    /// Projects it started that aren't archived, from `projects.list`.
    pub(crate) projects_made: usize,
    scroll: ScrollHandle,
}

impl LibrarianState {
    pub(crate) fn new() -> Self {
        LibrarianState { status: None, busy: HashSet::new(), projects_made: 0, scroll: ScrollHandle::new() }
    }
}

/// What the ⓘ beside the page's title says.
const MORE: &str = "It files each thread in a project, writes down where the project's up to, and renames threads that have wandered off from their title. It takes a look after a thread's first message and when a turn ends, then catches up at most every 20 minutes while the thread keeps going. A catch-up after only a quick reply is skipped without asking the model. Each look is one short call to Haiku, not the model your threads use, on the Claude login with the most room in the Runs on pool, and counts towards its usage. It waits when they're all out of room, and threads in a private folder are never sent. A thread you take out of a project is never put back in it.";

/// The projects a `projects.list` answer has that the librarian made and
/// aren't archived (`LibrarianStats`' "Projects it made").
pub(crate) fn projects_made(projects: &Value) -> usize {
    projects.as_array().into_iter().flatten().filter(|p| p["createdBy"].as_str() == Some("librarian") && !p["archived"].as_bool().unwrap_or(false)).count()
}

impl Threadbear {
    /// After the projects at connect (`loadProjects`).
    pub(crate) fn load_librarian(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(v) = call(client, "librarian.status", json!({})).await {
                this.update(cx, |this, cx| {
                    this.librarian.status = Some(Status::decode(&v["librarian"]));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Pause or Resume (`setLibrarianPaused`): shown at once, put back if
    /// the runner says no.
    pub(crate) fn set_librarian_paused(&mut self, paused: bool, cx: &mut Context<Self>) {
        if self.librarian.busy.contains("pause") {
            return;
        }
        let Some(before) = self.librarian.status.clone() else { return };
        let guess = before.setting(paused);
        self.librarian.status = Some(guess.clone());
        self.librarian.busy.insert("pause");
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "librarian.update", json!({ "paused": paused })).await;
            this.update(cx, |this, cx| {
                this.librarian.busy.remove("pause");
                match res {
                    Ok(v) => this.librarian.status = Some(Status::decode(&v["librarian"])),
                    Err(e) => {
                        if this.librarian.status.as_ref() == Some(&guess) {
                            this.librarian.status = Some(before);
                        }
                        this.show_error(e.message, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Look at everything (`runLibrarian`): a look at every thread it hasn't
    /// looked at yet, newest first.
    pub(crate) fn run_librarian(&mut self, cx: &mut Context<Self>) {
        if !self.librarian.busy.insert("run") {
            return;
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "librarian.run", json!({})).await;
            this.update(cx, |this, cx| {
                this.librarian.busy.remove("run");
                match res {
                    Ok(v) => this.librarian.status = Some(Status::decode(&v["librarian"])),
                    Err(e) => this.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // The sidebar's view.

    /// `LibrarianView`: what it's doing, then Next and Recent.
    pub(crate) fn render_librarian_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let now = now_ms();
        let side = p.side;
        let body: AnyElement = match self.librarian.status.clone() {
            None => div().px(px(OUTER_INSET)).child(div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(p.secondary).child("Loading…")).into_any_element(),
            Some(status) => {
                let mut list = v_flex().px(px(OUTER_INSET)).pb(px(32.)).child(self.render_librarian_state(&status, now, &p, cx));
                list = list.child(self.section_heading("librarian-next", "Next", "hourglass", status.queue.len(), false, false, None, &p, |_, _| {}, cx));
                for job in &status.queue {
                    list = list.child(self.render_queue_row(job, now, &p, cx));
                }
                if status.queue.is_empty() {
                    list = list.child(empty_note("Nothing waiting.", &p));
                }
                list = list.child(self.section_heading("librarian-recent", "Recent", "clock.arrow.circlepath", status.recent.len(), false, false, None, &p, |_, _| {}, cx));
                for entry in &status.recent {
                    list = list.child(self.render_entry_row(entry, now, &p, cx));
                }
                if status.recent.is_empty() {
                    list = list.child(empty_note("Nothing yet.", &p));
                }
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(div().id("librarian-list").size_full().overflow_y_scroll().track_scroll(&self.librarian.scroll).child(list))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(34.)).bg(linear_gradient(180., linear_color_stop(side.opacity(0.), 0.), linear_color_stop(side, 0.88))))
                    .into_any_element()
            }
        };
        v_flex().relative().size_full().bg(p.side).child(mode_header("Librarian", None, &p)).child(body).into_any_element()
    }

    /// `LibrarianState`: what it's doing now (with its mark in the rows'
    /// gutter), the look under way, today's numbers, Pause and Look at
    /// everything.
    fn render_librarian_state(&mut self, status: &Status, now: i64, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (fg, secondary, faint) = (p.fg, p.secondary, p.faint);
        let mark: AnyElement = match status.state {
            State::Working => Spinner::new().color(style::control_accent(cx)).with_size(px(12.)).into_any_element(),
            State::Paused => icon("pause.circle", 12., secondary).into_any_element(),
            State::Off => icon("exclamationmark.circle", 12., secondary).into_any_element(),
            State::Idle => div().size(px(6.)).rounded_full().bg(faint).into_any_element(),
        };
        let working = status.current.as_ref().filter(|_| status.state == State::Working).map(|job| {
            let why = rules::why(job);
            match job.started_ms {
                Some(started) => {
                    let ago = threadbear_core::sidebar::relative_short(started, &Local::now());
                    format!("{why} · started {}", if ago == "now" { "just now".to_owned() } else { format!("{ago} ago") })
                }
                None => why,
            }
        });
        let _ = now;
        let pausing = self.librarian.busy.contains("pause");
        let running = self.librarian.busy.contains("run");
        let paused = status.paused;
        let pause_tip = if paused { "Let it look at threads again" } else { "Stop looking at threads; what's queued waits" };
        let buttons = h_flex()
            .gap(px(8.))
            .pt(px(4.))
            .child(
                div().id("librarian-pause").test_support().aria_label(pause_tip.to_owned()).child(
                    // Small and filled, as the Swift view's `.controlSize(.small)`.
                    Button::new("librarian-pause-button")
                        .label(if paused { "Resume" } else { "Pause" })
                        .xsmall()
                        .secondary()
                        .disabled(pausing)
                        .tooltip(pause_tip)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_librarian_paused(!paused, cx))),
                ),
            )
            .child(
                div().id("librarian-run").test_support().aria_label("Queue a look at every thread it hasn't looked at yet, newest first".to_owned()).child(
                    Button::new("librarian-run-button")
                        .label("Look at everything")
                        .xsmall()
                        .secondary()
                        .loading(running)
                        .disabled(running)
                        .tooltip("Queue a look at every thread it hasn't looked at yet, newest first")
                        .on_click(cx.listener(|this, _, _, cx| this.run_librarian(cx))),
                ),
            );
        let today_tip = "Today's looks, quiet catch-ups it skipped without asking the model, and tokens used on your Claude login";
        v_flex()
            .gap(px(6.))
            .pl(px(LEADING))
            .pr(px(TRAILING))
            .pt(px(8.))
            .child(
                div()
                    .relative()
                    // In the rows' leading gutter, as a thread's unread dot
                    // is, so the words line up with the titles below; on the
                    // first line when they wrap. Outside the clamped words,
                    // which would cut it off.
                    .child(div().absolute().left(px(-(LEADING + 12.) / 2.)).top(px(2.)).size(px(12.)).flex().items_center().justify_center().child(mark))
                    .child(div().text_size(px(style::size::SIDEBAR_TITLE)).line_height(px(17.)).text_color(fg).line_clamp(2).child(rules::state_text(status))),
            )
            .when_some(working, |s, line| s.child(div().text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(secondary).line_clamp(2).child(line)))
            .child(tipped!(div().text_size(px(style::size::SIDEBAR_META)).line_height(px(14.)).text_color(faint), "librarian-today", today_tip).child(rules::today(status)))
            .child(buttons)
            .into_any_element()
    }

    /// A thread's title that opens it, or plain words for one that's gone
    /// (`ThreadLink`).
    fn thread_link(&mut self, key: String, thread_id: &str, title: &str, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let shown = match self.threads.get(thread_id) {
            Some(t) if t.title.is_empty() => "Untitled".to_owned(),
            Some(t) => t.title.clone(),
            None if title.is_empty() => "A thread that's gone".to_owned(),
            None => title.to_owned(),
        };
        if !self.threads.contains_key(thread_id) {
            return div().min_w_0().truncate().child(shown).into_any_element();
        }
        let id = thread_id.to_owned();
        let underline = p.secondary;
        tipped!(div().min_w_0().truncate().cursor_pointer().hover(move |s| s.underline().text_decoration_color(underline)), ElementId::Name(key.into()), "Open the thread")
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open(id.clone(), window, cx)))
            .child(shown)
            .into_any_element()
    }

    /// `QueueRow`: a look it will take, the thread (which opens), why and when.
    fn render_queue_row(&mut self, job: &Job, now: i64, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let due = rules::due(job.due_ms, now);
        let full = job.due_ms.map(|at| threadbear_core::schedules::full_time(at, &Local)).unwrap_or_default();
        let link = self.thread_link(format!("librarian-next-{}", job.thread_id), &job.thread_id, &job.thread_title, p, cx);
        v_flex()
            .gap(px(3.))
            .pt(px(7.))
            .pb(px(7.))
            .pl(px(LEADING))
            .pr(px(TRAILING))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().text_size(px(style::size::SIDEBAR_TITLE)).line_height(px(17.)).text_color(p.fg).child(link))
                    .child(
                        div()
                            .id(ElementId::Name(format!("librarian-due-{}", job.thread_id).into()))
                            .flex_none()
                            .text_size(px(style::size::SIDEBAR_META))
                            .line_height(px(14.))
                            .text_color(p.faint)
                            .tip(full.clone())
                            .child(due),
                    ),
            )
            .child(div().truncate().text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(p.secondary).child(rules::why(job)))
            .into_any_element()
    }

    /// `EntryRow`: something it did, to which thread and projects, and when.
    fn render_entry_row(&mut self, entry: &Entry, now: i64, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let _ = now;
        let words = if entry.text.is_empty() { rules::outcome_words(entry.outcome).to_owned() } else { entry.text.clone() };
        let color = match entry.outcome {
            Outcome::Failed => p.failed,
            Outcome::Nothing | Outcome::Skipped => p.secondary,
            _ => p.fg,
        };
        let when = entry.at_ms.map(|at| (threadbear_core::sidebar::relative_short(at, &Local::now()), threadbear_core::schedules::full_time(at, &Local)));
        let titles: Vec<(String, String)> = entry.project_ids.iter().filter_map(|id| self.all_projects.iter().find(|(pid, _, _)| pid == id).map(|(pid, title, _)| (pid.clone(), title.clone()))).collect();
        // Why it filed the thread, then what prompted the look.
        let mut tip = rules::filing_reasons(entry, |id| titles.iter().find(|(pid, _)| pid == id).map(|(_, t)| t.clone()));
        tip.push(rules::entry_help(entry));
        let tip = tip.join("\n");
        // The thread, then the projects it touched. Tidying a project's
        // notes has no thread, just the project. A project's page isn't in
        // this app yet, so its name is plain words.
        let mut place = h_flex().min_w_0().items_center().gap(px(4.)).text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(p.secondary);
        // The thread stays whole before the projects are cut short
        // (`layoutPriority(1)` in the Swift row), up to most of the line.
        if !entry.thread_id.is_empty() {
            let link = self.thread_link(format!("librarian-entry-{}", entry.id), &entry.thread_id, &entry.thread_title, p, cx);
            place = place.child(div().flex_none().max_w(relative(0.8)).child(link));
        }
        for (n, (_, title)) in titles.iter().enumerate() {
            if n > 0 || !entry.thread_id.is_empty() {
                place = place.child(div().flex_none().text_color(p.faint).child("·"));
            }
            place = place.child(div().min_w_0().truncate().child(title.clone()));
        }
        tipped!(v_flex().gap(px(3.)).pt(px(7.)).pb(px(7.)).pl(px(LEADING)).pr(px(TRAILING)), ElementId::Name(format!("librarian-entry-row-{}", entry.id).into()), tip)
            .child(
                h_flex()
                    .items_start()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().text_size(px(style::size::SIDEBAR_TITLE)).line_height(px(17.)).text_color(color).line_clamp(2).child(words))
                    .when_some(when, |s, (ago, full)| {
                        s.child(
                            div()
                                .id(ElementId::Name(format!("librarian-at-{}", entry.id).into()))
                                .flex_none()
                                .pt(px(2.))
                                .text_size(px(style::size::SIDEBAR_META))
                                .line_height(px(14.))
                                .text_color(p.faint)
                                .tip(full.clone())
                                .child(ago),
                        )
                    }),
            )
            .child(place)
            .into_any_element()
    }

    // The page beside it.

    /// `LibrarianHome`: the bear at its card catalogue, what it's for, what
    /// it's doing, its numbers, then its settings.
    pub(crate) fn render_librarian_home(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.librarian.status.clone();
        let threads: Vec<&threadbear_core::inbox::ThreadSummary> = self.threads.values().collect();
        let stats = status.as_ref().map(|s| rules::stats(s, self.librarian.projects_made, &threads)).unwrap_or_default();
        let mut about = v_flex().gap(px(12.)).child(home_title("Librarian", "Keeps your projects tidy as your threads go.", MORE, cx));
        if let Some(status) = &status {
            about = about.child(home_status_line(&rules::state_text(status), status.state == State::Working, cx));
        }
        let below = status.as_ref().map(|status| self.render_librarian_settings(status, cx));
        self.render_home_page(Art::Librarian, about.into_any_element(), &stats, below, cx)
    }

    /// `LibrarianSettings`: whether it's looking at all, and which pool's
    /// Claude logins it runs on (the runner's small-jobs setting, which
    /// titles share), with the rest in Settings.
    fn render_librarian_settings(&mut self, status: &Status, cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        let pausing = self.librarian.busy.contains("pause");
        let paused = status.paused;
        let label = |text: &'static str| div().w(px(110.)).flex_none().text_size(px(style::size::LABEL)).line_height(px(16.)).text_color(secondary).child(text);
        let switch_tip = if paused { "Paused: what's queued waits. Turn it on to carry on" } else { "Turn it off to pause; what's queued waits" };
        let switch = div().id("librarian-switch").test_support().aria_label(switch_tip.to_owned()).tip(switch_tip).child(
            Switch::new("librarian-look")
                .checked(!paused)
                .disabled(pausing)
                .small()
                .on_click(cx.listener(move |this, on: &bool, _, cx| this.set_librarian_paused(!*on, cx))),
        );
        let mut grid = v_flex().gap(px(12.)).child(h_flex().items_center().gap(px(16.)).child(label("Look at threads")).child(switch));
        if let Some(picker) = self.render_small_jobs_pool(cx) {
            grid = grid.child(h_flex().items_center().gap(px(16.)).child(label("Runs on")).child(picker));
        }
        v_flex()
            .pt(px(8.))
            .gap(px(12.))
            .child(div().text_size(px(17.)).line_height(px(21.)).font_semibold().child("Settings"))
            .child(grid)
            .child(
                div().id("librarian-more-settings").test_support().aria_label("Private folders, whose threads are never sent anywhere".to_owned()).child(
                    Button::new("librarian-more-settings-button")
                        .label("More in Settings…")
                        .small()
                        .outline()
                        .tooltip("Private folders, whose threads are never sent anywhere")
                        // Settings › Models, where the private folders are (`openModelSettings`).
                        .on_click(cx.listener(|this, _, _, cx| this.open_settings(Some(crate::settings_window::Tab::Models), cx))),
                ),
            )
            .into_any_element()
    }

    /// Runs on: the pools with a Claude login (`claudePools`), the one set
    /// ticked; one that's set but can't take a look shown so it still says
    /// what's set. None until the runner's settings have come, or from a
    /// runner too old to have the setting.
    fn render_small_jobs_pool(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let settings = self.runner_settings.as_ref()?;
        let models = settings.get("models")?;
        let set = models.get("pool")?.as_str().filter(|p| !p.is_empty()).map(str::to_owned);
        let small: Vec<String> = self.small_job_providers();
        let pools: Vec<(String, String, String)> = self
            .logins
            .pools
            .iter()
            .filter(|pool| pool.account_ids.iter().any(|id| self.logins.account(id).is_some_and(|a| small.iter().any(|p| p == a.provider_id()))))
            .map(|pool| (pool.id.clone(), pool.title.clone(), pool.icon.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| "square.stack".into())))
            .collect();
        let current = match &set {
            Some(id) => pools.iter().find(|p| &p.0 == id).map_or_else(|| format!("{id}, which has no Claude login"), |p| p.1.clone()),
            None => "No pool".to_owned(),
        };
        let app = cx.entity().downgrade();
        let tip = "Which pool's Claude logins it uses, the one with the most room. It waits when they're all out of room. Titles use the same pool (Settings › Models)";
        let choices = pools.clone();
        let set_for_menu = set.clone();
        Some(
            div()
                .id("librarian-pool")
                .test_support()
                .aria_label(tip.to_owned())
                .child(Button::new("librarian-pool-button").label(current).small().outline().dropdown_caret(true).tooltip(tip).dropdown_menu(move |menu: PopupMenu, _, _| {
                    let mut menu = menu;
                    for (id, title, symbol) in &choices {
                        let (app, id) = (app.clone(), id.clone());
                        let checked = set_for_menu.as_deref() == Some(id.as_str());
                        let mut item = PopupMenuItem::new(title.clone()).checked(checked).on_click(move |_, _, cx| {
                            let id = id.clone();
                            app.update(cx, |app, cx| app.update_runner_settings(json!({ "models": { "pool": id } }), cx)).ok();
                        });
                        if !checked {
                            item = item.icon(icons::symbol(symbol));
                        }
                        menu = menu.item(item);
                    }
                    menu
                }))
                .into_any_element(),
        )
    }

    /// The page in the main panel, for the dump (`homePage`).
    #[cfg(feature = "automation")]
    pub(crate) fn librarian_home_dump(&self) -> Value {
        let threads: Vec<&threadbear_core::inbox::ThreadSummary> = self.threads.values().collect();
        let status = self.librarian.status.as_ref();
        let stats = status.map(|s| rules::stats(s, self.librarian.projects_made, &threads)).unwrap_or_default();
        json!({
            "title": "Librarian",
            "status": status.map(rules::state_text),
            "poke": self.librarian_pokes.line,
            "stats": stats.iter().map(|s| json!({ "value": s.value, "label": s.label })).collect::<Vec<_>>(),
        })
    }

    /// The Librarian view in the dump, in the Swift app's shape: Next and
    /// Recent as the sidebar's sections, looks as their rows (AppDump's
    /// `.librarian`), and its state in words.
    #[cfg(feature = "automation")]
    pub(crate) fn librarian_dump(&self) -> (Value, Value) {
        let Some(status) = &self.librarian.status else { return (json!([]), Value::Null) };
        let row = |id: &str, title: &str, status: &str| json!({ "id": id, "title": title, "status": status, "needsYou": false, "unread": false, "hovered": false });
        let reason = |r: threadbear_core::librarian::Reason| -> &'static str {
            use threadbear_core::librarian::Reason::*;
            match r {
                FirstMessage => "first-message",
                FirstTurn => "first-turn",
                NewFolder => "new-folder",
                Moved => "moved",
                CatchUp => "catch-up",
                Asked => "asked",
                Notes => "notes",
            }
        };
        let outcome = |o: Outcome| -> &'static str {
            match o {
                Outcome::Joined => "joined",
                Outcome::Created => "created",
                Outcome::Updated => "updated",
                Outcome::Left => "left",
                Outcome::Nothing => "nothing",
                Outcome::Skipped => "skipped",
                Outcome::Failed => "failed",
                Outcome::Other => "other",
            }
        };
        let sections = json!([
            { "name": "Next", "threads": status.queue.iter().map(|j| row(&j.thread_id, &j.thread_title, reason(j.reason))).collect::<Vec<_>>() },
            { "name": "Recent", "threads": status.recent.iter().map(|e| row(&e.thread_id, &e.text, outcome(e.outcome))).collect::<Vec<_>>() },
        ]);
        (sections, json!(rules::state_text(status)))
    }
}

/// `EmptyNote`: under a heading with nothing in it.
fn empty_note(text: &'static str, p: &Palette) -> impl IntoElement {
    div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(p.secondary).child(text)
}
