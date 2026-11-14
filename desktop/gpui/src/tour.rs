// The first-run tour and the pages' one-card intros, drawn: a port of the
// Swift app's TourOverlay, TourCard, TourGlow, TourArrow and Spotlight
// (app/Sources/Threadbear/Views/Tour.swift). The steps, their words and where
// the card goes are threadbear_core::tour; this finds the parts of the window
// they point at, dims the window with that part cut out, puts the card beside
// it with an arrow, and does what each step does (opens a sample, draws a
// row hovered, writes the first message).
//
// The parts are found two ways. A whole panel (Threads, Highlights, the main
// panel, Usage and the rest under the threads, Finish setting up) is the
// dock's own frame for its group, bar and all, so the tour finds it wherever
// the dock put it. The smaller parts (the composer, the action chips, a
// sample's row and its buttons, the activity bar's view icons, the rail's
// Highlights icon) are measured where they're drawn, with `mark`, only while
// the tour's up. Those are cleared on each step and the window drawn again, so
// a part that's gone (a panel closed, the samples removed) isn't lit.
use crate::app::Threadbear;
use crate::shell::{self, SidebarMode};
use crate::style::{self, px};
use crate::tips::Tip as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::tour::{self as rules, Edge, Finished, Intro, Rect, Step, Target, Task as TourTask};

const BEAR_WAVE: &[u8] = include_bytes!("../icons/bear-wave.svg");
/// The card's width at 100%, as the Swift card's `zoomed(320)`.
const CARD_WIDTH: f32 = 320.;
/// The arrow's half-width and depth.
const ARROW: f32 = 10.;
/// The hole's rounded corners.
const CORNER: f32 = 10.;

thread_local! {
    /// The tour's up: the marks measure only then.
    static ON: Cell<bool> = const { Cell::new(false) };
    /// Where each marked part was last drawn, in the window's points.
    static FRAMES: RefCell<HashMap<Target, Rect>> = RefCell::new(HashMap::new());
    /// Where the dock is in the window: its frames are from its own corner.
    static DOCK_ORIGIN: Cell<(f32, f32)> = const { Cell::new((0., 0.)) };
    /// The thread open in the main panel ("" for the new-thread page), so
    /// only its composer is the one lit, not a thread's in a panel of its own.
    static OPEN: RefCell<String> = const { RefCell::new(String::new()) };
}

fn rect_of(bounds: Bounds<Pixels>) -> Rect {
    Rect::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y), f32::from(bounds.size.width), f32::from(bounds.size.height))
}

fn record(target: Target, rect: Rect, window: &mut Window) {
    let changed = FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        if f.get(&target) == Some(&rect) {
            false
        } else {
            f.insert(target, rect);
            true
        }
    });
    // The overlay is drawn before the marks are measured: once more, with them.
    if changed && ON.with(Cell::get) {
        window.refresh();
    }
}

/// Measures the part of the window it's put in (as a child, filling it) as
/// `target`.
pub(crate) fn mark(target: Target) -> Option<AnyElement> {
    // Always measured, so the tour knows at its start what's on screen; the
    // window's only drawn again for a change while it's up.
    Some(canvas(move |bounds, window, _| record(target.clone(), rect_of(bounds), window), |_, _, _, _| {}).absolute().top_0().left_0().size_full().into_any_element())
}

/// The composer's mark: only the main panel's (`thread` None is the
/// new-thread page's).
pub(crate) fn mark_composer(thread: Option<&str>) -> Option<AnyElement> {
    let open = OPEN.with(|o| o.borrow().clone());
    (thread.unwrap_or("") == open).then(|| mark(Target::Composer)).flatten()
}

/// Where the dock sits, from the shell, every frame.
pub(crate) fn dock_origin_mark() -> AnyElement {
    canvas(|bounds, _, _| DOCK_ORIGIN.with(|o| o.set((f32::from(bounds.origin.x), f32::from(bounds.origin.y)))), |_, _, _, _| {}).absolute().top_0().left_0().size_full().into_any_element()
}

/// The tour's state in the app (`Threadbear::tour`).
#[derive(Default)]
pub(crate) struct TourState {
    pub(crate) tour: rules::Tour,
    /// The sample threads this run asked the runner for, in its order.
    pub(crate) samples: Vec<String>,
    /// The thread the tour had them send.
    pub(crate) first_thread: Option<String>,
    threads_at_start: HashSet<String>,
    /// A hands-on step that hasn't taken after `rules::PATIENCE_MS`: Next
    /// shows anyway, so nobody's stuck.
    waited: bool,
    patience: Option<gpui_kit::Task<()>>,
    /// The card's size as last drawn, for placing it.
    card_size: Rc<Cell<(f32, f32)>>,
    focus: Option<FocusHandle>,
    /// The intros: off in a scripted app (its pages would be covered) until
    /// the automation's `tour` turns them on.
    pub(crate) intros_off: bool,
    /// What the intros watch, as of the last frame: the sidebar's view and
    /// the panels under the threads.
    watched: Option<(SidebarMode, bool, bool, bool)>,
    pools_seen: usize,
    /// The first-run check has been made this launch.
    checked_new: bool,
    /// When the step on show came up.
    step_at: Option<std::time::Instant>,
}

use std::rc::Rc;

impl TourState {
    pub(crate) fn new() -> Self {
        TourState { intros_off: crate::scripted::scripted(), card_size: Rc::new(Cell::new((CARD_WIDTH, 160.))), ..Default::default() }
    }
}

/// ~/ThreadbearThreads/Welcome: Threadbear's own folder, nowhere macOS asks
/// about. A scripted app keeps it in its runner's data folder, away from the
/// real one.
fn welcome_folder() -> String {
    if crate::scripted::scripted() {
        return threadbear_client::config::RunnerConfig::from_env().home.join("Welcome").to_string_lossy().into_owned();
    }
    crate::installed::home_dir().join("ThreadbearThreads").join("Welcome").to_string_lossy().into_owned()
}

impl Threadbear {
    /// Where a part of the window is: a panel's from the dock, the rest as
    /// last measured.
    fn tour_frame_of(&self, target: &Target, cx: &App) -> Option<Rect> {
        let panel = match target {
            Target::Threads => Some(shell::THREADS),
            Target::Highlights => Some(shell::HIGHLIGHTS),
            Target::MainPanel => Some(shell::MAIN),
            Target::SetupPanel => Some(shell::SETUP),
            Target::Panel("usage") => Some(shell::USAGE),
            Target::Panel("updates") => Some(shell::UPDATES),
            Target::Panel("connectors") => Some(shell::CONNECTORS),
            _ => None,
        };
        let Some(panel) = panel else { return FRAMES.with(|f| f.borrow().get(target).copied()) };
        let controller = &self.dock.read(cx).controller;
        if !controller.is_showing(panel) || controller.layout.is_floating(panel) || !self.panel_available(panel, cx) {
            return None;
        }
        let group = controller.layout.group_containing(panel)?;
        let r = controller.frames.get(&group.id)?;
        let (x, y) = DOCK_ORIGIN.with(Cell::get);
        Some(Rect::new(x + r.x as f32, y + r.y as f32, r.width as f32, r.height as f32))
    }

    fn tour_frame(&self, step: &Step, cx: &App) -> Option<Rect> {
        rules::Tour::frame(step, &|t| self.tour_frame_of(t, cx))
    }

    /// The first time the app has connected on this Mac (or with this data
    /// folder): the tour, by itself, with no Skip. Never in a scripted run
    /// (the automation's `tour` starts it there), and not before the Swift
    /// app's has been looked for (swift_import.rs).
    pub(crate) fn tour_start_if_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A spec can ask a scripted app to behave as a new install does.
        let scripted = crate::scripted::scripted() && std::env::var_os("THREADBEAR_TOUR_AUTOSTART").is_none();
        if self.tour.checked_new || scripted {
            return;
        }
        self.tour.checked_new = true;
        // Opened before the runner had made its data folder (a fresh
        // install), the look for the Swift app's tour couldn't be saved:
        // again, now the folder's there.
        if self.settings.saved.swift_tour_import.is_none() {
            crate::swift_import::tour_once(&mut self.settings);
        }
        if self.settings.saved.tour_seen == Some(true) || self.settings.saved.swift_tour_import.is_none() || self.tour.tour.is_on() {
            return;
        }
        // Once the window's drawn with their threads in it.
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(600)).await;
            this.update_in(cx, |this, window, cx| this.tour_start(false, window, cx)).ok();
        })
        .detach();
    }

    /// Starts it: Help > Take the Tour (skippable), or the first run.
    pub(crate) fn tour_start(&mut self, skippable: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Samples to try things on, from the runner, in the Welcome folder
        // with its example patch (trusted, so Roll a die shows without asking).
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = crate::call(client, "threads.samples", json!({ "cwd": welcome_folder() })).await;
            this.update_in(cx, |this, window, cx| {
                match res {
                    Ok(v) => {
                        let samples: Vec<ThreadSummary> = v["threads"].as_array().into_iter().flatten().filter_map(|t| serde_json::from_value(t.clone()).ok()).collect();
                        this.tour.samples = samples.iter().map(|t| t.id.clone()).collect();
                        for t in samples {
                            this.threads.insert(t.id.clone(), std::sync::Arc::new(t));
                        }
                        // The step on show may want one open or hovered.
                        if this.tour.tour.is_on() && this.tour.tour.intro.is_none() {
                            this.tour_apply_step(window, cx);
                        }
                    }
                    // A runner from before `threads.samples`: on without them.
                    Err(e) => eprintln!("threads.samples failed: {}", e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        ON.with(|o| o.set(true));
        let frames: HashMap<Target, Option<Rect>> = rules::steps().iter().flat_map(|s| s.targets.clone()).map(|t| (t.clone(), self.tour_frame_of(&t, cx))).collect();
        self.tour.tour.start(skippable, &|t| frames.get(t).copied().flatten());
        self.tour_go(window, cx);
    }

    pub(crate) fn tour_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tour.tour.is_on() {
            return;
        }
        if self.tour.tour.next() {
            self.tour_go(window, cx);
        } else {
            self.tour_finish(cx);
        }
    }

    pub(crate) fn tour_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tour.tour.back();
        self.tour_go(window, cx);
    }

    /// Done or skipped: the tour doesn't come up by itself again and the
    /// samples go; an intro never shows again.
    pub(crate) fn tour_finish(&mut self, cx: &mut Context<Self>) {
        match self.tour.tour.finish() {
            Some(Finished::Intro(intro)) => {
                let seen = self.settings.saved.tour_intros_seen.get_or_insert_with(Vec::new);
                if !seen.iter().any(|s| s == intro.name()) {
                    seen.push(intro.name().to_owned());
                }
            }
            Some(Finished::Tour) => {
                self.settings.saved.tour_seen = Some(true);
                self.sidebar.hover_override = None;
                if !std::mem::take(&mut self.tour.samples).is_empty() {
                    self.tour_remove_samples(cx);
                }
            }
            None => return,
        }
        self.tour.patience = None;
        ON.with(|o| o.set(false));
        FRAMES.with(|f| f.borrow_mut().clear());
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// Deletes the samples, archived and hibernating ones too. The runner says
    /// each is gone, which takes it off the list.
    fn tour_remove_samples(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |_, _| {
            if let Err(e) = crate::call(client, "threads.samples", json!({ "cwd": welcome_folder(), "remove": true })).await {
                eprintln!("removing the tour's samples failed: {}", e.message);
            }
        })
        .detach();
    }

    /// On to the step now on show: what it opens or hovers, its task, the
    /// keyboard, and the patience timer.
    fn tour_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tour.tour.task_done = false;
        self.tour.waited = false;
        self.tour.step_at = Some(std::time::Instant::now());
        // Measured afresh: what the last step lit may have gone.
        FRAMES.with(|f| f.borrow_mut().clear());
        self.tour_apply_step(window, cx);
        let task = self.tour.tour.current().and_then(|s| s.task);
        if task == Some(TourTask::SendFirst) {
            self.tour.threads_at_start = self.threads.keys().cloned().collect();
            self.new_thread(cx);
            let composer = self.ensure_new_composer(window, cx);
            composer.update(cx, |c, cx| c.replace_text(rules::FIRST_MESSAGE.to_owned(), cx));
        } else {
            // Return and Esc are the tour's while it's up, not a send or an
            // empty line in a composer.
            let focus = self.tour.focus.get_or_insert_with(|| cx.focus_handle()).clone();
            window.focus(&focus, cx);
            // And AppKit's keyboard with it, or a web panel's page keeps it.
            crate::web_panels::keyboard_to_app(window);
        }
        if task.is_some() {
            self.tour.patience = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(rules::PATIENCE_MS)).await;
                this.update(cx, |this, cx| {
                    this.tour.waited = true;
                    cx.notify();
                })
                .ok();
            }));
        } else {
            self.tour.patience = None;
        }
        window.refresh();
        cx.notify();
    }

    /// The sample a step opens, and the row it draws hovered.
    fn tour_apply_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(step) = self.tour.tour.current().cloned() else { return };
        if step.opens_sample {
            let now = crate::now_ms();
            // The first still in the inbox: the one that needs you, unless
            // it's been put away.
            let open = self.tour.samples.iter().find(|id| self.threads.get(*id).is_some_and(|t| !t.is_archived() && !t.snoozed(now))).cloned();
            if let Some(id) = open {
                self.set_mode(SidebarMode::Inbox, cx);
                if self.selected.as_deref() != Some(id.as_str()) {
                    self.open(id, window, cx);
                }
            }
        }
        // Drawn without the pointer, as the screenshot run does; None puts
        // the row back.
        self.sidebar.hover_override = step.hovers.and_then(|title| self.tour.samples.iter().find(|id| self.threads.get(*id).is_some_and(|t| t.title == title)).cloned());
    }

    /// Whether the step's task is done, as the threads change: the first
    /// thread sent moves on to what's left to set up, or ends the tour if
    /// nothing is.
    fn tour_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(TourTask::SendFirst) = self.tour.tour.current().and_then(|s| s.task) else { return };
        if self.tour.tour.task_done {
            return;
        }
        let Some(id) = self.threads.keys().find(|k| !self.tour.threads_at_start.contains(*k) && !self.tour.samples.contains(*k)).cloned() else { return };
        self.tour.first_thread = Some(id);
        self.tour.tour.task_done = true;
        if self.setup_shows_icon() {
            self.show_setup(cx);
            self.tour_next(window, cx);
        } else {
            self.tour_finish(cx);
        }
    }

    /// The first thread they sent has failed.
    fn tour_first_failed(&self) -> bool {
        self.tour.first_thread.as_ref().and_then(|id| self.threads.get(id)).is_some_and(|t| t.status == "error")
    }

    /// A page's or panel's intro the first time it's opened, once it's drawn
    /// for the card to point at.
    pub(crate) fn tour_introduce_soon(&mut self, intro: Intro, window: &mut Window, cx: &mut Context<Self>) {
        if self.tour.intros_off {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            this.update_in(cx, |this, window, cx| this.tour_introduce(intro, window, cx)).ok();
        })
        .detach();
    }

    fn tour_introduce(&mut self, intro: Intro, window: &mut Window, cx: &mut Context<Self>) {
        let seen = self.settings.saved.tour_intros_seen.as_ref().is_some_and(|s| s.iter().any(|n| n == intro.name()));
        if self.tour.tour.introduce(intro, seen, !self.tour.intros_off) {
            ON.with(|o| o.set(true));
            self.tour_go(window, cx);
        }
    }

    /// Each frame, from the shell: the open thread for the composer's mark,
    /// the first thread's send, and the intros' triggers (the sidebar's view
    /// changing, a panel under the threads coming on show, a second login in
    /// a pool).
    pub(crate) fn tour_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        OPEN.with(|o| *o.borrow_mut() = if self.home_page().is_some() { "-".into() } else { self.selected.clone().unwrap_or_default() });
        // Someone's first launch: once the runner's there.
        if !self.tour.checked_new && matches!(self.status, threadbear_client::runner::Status::Connected { .. }) {
            cx.defer_in(window, |this, window, cx| this.tour_start_if_new(window, cx));
        }
        if self.tour.tour.current().is_some_and(|s| s.task.is_some()) && !self.tour.tour.task_done {
            cx.defer_in(window, |this, window, cx| this.tour_check(window, cx));
        }
        let dock = &self.dock.read(cx).controller;
        let now = (self.mode, dock.is_showing(shell::USAGE), dock.is_showing(shell::UPDATES), dock.is_showing(shell::CONNECTORS));
        let was = self.tour.watched.replace(now);
        if let Some(was) = was {
            let mut due = vec![];
            if was.0 != now.0 {
                match now.0 {
                    SidebarMode::Scheduled => due.push(Intro::Scheduled),
                    SidebarMode::Projects => due.push(Intro::Projects),
                    SidebarMode::Librarian => due.push(Intro::Librarian),
                    SidebarMode::Inbox => {}
                }
            }
            for (on, w, intro) in [(now.1, was.1, Intro::Usage), (now.2, was.2, Intro::Updates), (now.3, was.3, Intro::Connectors)] {
                if on && !w {
                    due.push(intro);
                }
            }
            for intro in due {
                cx.defer_in(window, move |this, window, cx| this.tour_introduce_soon(intro, window, cx));
            }
        }
        let pools = self.logins.pools.iter().filter(|p| p.account_ids.len() >= 2).count();
        if pools != self.tour.pools_seen {
            self.tour.pools_seen = pools;
            if rules::pools_intro_due(&self.logins.pools, self.settings.saved.tour_seen == Some(true)) {
                cx.defer_in(window, |this, window, cx| this.tour_introduce_soon(Intro::Pools, window, cx));
            }
        }
    }

    // Drawing it.

    /// The dimmed window with the step's part cut out of it, the card and
    /// its arrow, over everything (`TourOverlay`).
    pub(crate) fn render_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let index = self.tour.tour.step?;
        let step = self.tour.tour.current()?.clone();
        let count = self.tour.tour.shown.len();
        let size = window.viewport_size();
        let bounds = Rect::new(0., 0., f32::from(size.width), f32::from(size.height));
        // A few points round it, and never past the window's edge.
        let hole = self.tour_frame(&step, cx).map(|r| r.outset(4.).intersection(&bounds)).filter(|r| r.w > 1. && r.h > 1.);
        // A part that's on its way (a panel gliding in under the threads, a
        // thread's composer still opening) has no frame yet, and the dock's
        // frames don't redraw the app: look again next frame till it's there.
        // Only for a few seconds: a part that never shows (a panel closed
        // since) shouldn't keep the window drawing.
        if hole.is_none() && !step.targets.is_empty() && self.tour.step_at.is_some_and(|at| at.elapsed() < Duration::from_secs(3)) {
            window.request_animation_frame();
        }
        let card_size = self.tour.card_size.get();
        let (origin, edge) = rules::card_placement(hole, card_size, bounds, &step.edges, 14., 12.);
        let dark = cx.theme().is_dark();
        // Darker in dark mode, where 0.4 barely showed against the dark panels.
        let dim = hsla(0., 0., 0., if dark { 0.55 } else { 0.35 });
        let card_bg = cx.theme().background;
        let hands_on = step.task.is_some();
        let raw = gpui_kit::px;

        let mut layer = div().id("tour").absolute().top_0().left_0().w(raw(bounds.w)).h(raw(bounds.h));
        // The dim, with a rounded hole.
        let shape_hole = hole;
        layer = layer.child(
            canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    let mut path = PathBuilder::fill();
                    let (x0, y0, x1, y1) = (b.origin.x, b.origin.y, b.origin.x + b.size.width, b.origin.y + b.size.height);
                    path.move_to(point(x0, y0));
                    path.line_to(point(x1, y0));
                    path.line_to(point(x1, y1));
                    path.line_to(point(x0, y1));
                    path.close();
                    if let Some(h) = shape_hole {
                        rounded_rect(&mut path, h, CORNER);
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, dim);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        // Clicks on the window behind wait till the tour's done, except in
        // the lit part of a hands-on step (the composer): four blocks round
        // the hole, and one over it when there's nothing to do there.
        let block = |id: &'static str, r: Rect| div().id(id).absolute().left(raw(r.x)).top(raw(r.y)).w(raw(r.w.max(0.))).h(raw(r.h.max(0.))).occlude().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        match hole {
            Some(h) => {
                layer = layer
                    .child(block("tour-block-top", Rect::new(0., 0., bounds.w, h.y)))
                    .child(block("tour-block-bottom", Rect::new(0., h.max_y(), bounds.w, bounds.h - h.max_y())))
                    .child(block("tour-block-left", Rect::new(0., h.y, h.x, h.h)))
                    .child(block("tour-block-right", Rect::new(h.max_x(), h.y, bounds.w - h.max_x(), h.h)));
                if !hands_on {
                    layer = layer.child(block("tour-block-hole", h));
                }
                // A soft glow round the lit part, so the eye goes there (Mike,
                // 30 Oct: the card alone didn't say what it meant), breathing
                // in and out (still with Reduce Motion).
                let accent = style::accent(cx);
                // GPUI fills a shadow under the whole box, which would cover
                // the lit part, so the glow is a second ring outside the
                // first that brightens and fades.
                let ring = div().absolute().left(raw(h.x)).top(raw(h.y)).w(raw(h.w)).h(raw(h.h)).rounded(raw(CORNER)).border(raw(2.)).border_color(accent);
                let halo = move |t: f32| div().absolute().left(raw(h.x - 4.)).top(raw(h.y - 4.)).w(raw(h.w + 8.)).h(raw(h.h + 8.)).rounded(raw(CORNER + 4.)).border(raw(3.)).border_color(accent.opacity(0.15 + 0.35 * t));
                let halo: AnyElement = if cx.reduce_motion() {
                    halo(0.4).into_any_element()
                } else {
                    div().absolute().top_0().left_0().size_full().child(halo(0.)).with_animation(ElementId::Name(format!("tour-glow-{index}").into()), Animation::new(Duration::from_millis(2200)).repeat(), move |_, t| {
                        let t = (t * std::f32::consts::PI).sin();
                        div().absolute().top_0().left_0().size_full().child(halo(t))
                    })
                    .into_any_element()
                };
                layer = layer.child(halo);
                layer = layer.child(ring);
            }
            None => layer = layer.child(block("tour-block", bounds)),
        }
        // The card's pointer: a small triangle on its side facing the part,
        // in the card's own colour, so the two read as one shape.
        if let (Some(h), Some(edge)) = (hole, edge) {
            let card = Rect::new(origin.0, origin.1, card_size.0, card_size.1);
            let (ax, ay) = rules::arrow_anchor(edge, card, h);
            layer = layer.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let s = ARROW;
                        let pts: [(f32, f32); 3] = match edge {
                            // The card's right of the part: the arrow points left.
                            Edge::Right => [(ax + 1., ay - s), (ax - s, ay), (ax + 1., ay + s)],
                            Edge::Left => [(ax - 1., ay - s), (ax + s, ay), (ax - 1., ay + s)],
                            Edge::Below => [(ax - s, ay + 1.), (ax, ay - s), (ax + s, ay + 1.)],
                            Edge::Above => [(ax - s, ay - 1.), (ax, ay + s), (ax + s, ay - 1.)],
                        };
                        let mut path = PathBuilder::fill();
                        path.move_to(point(raw(pts[0].0), raw(pts[0].1)));
                        path.line_to(point(raw(pts[1].0), raw(pts[1].1)));
                        path.line_to(point(raw(pts[2].0), raw(pts[2].1)));
                        path.close();
                        if let Ok(path) = path.build() {
                            window.paint_path(path, card_bg);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        let card = self.render_tour_card(&step, index, count, cx);
        layer = layer.child(div().absolute().left(raw(origin.0)).top(raw(origin.1)).child(card));
        let focus = self.tour.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let layer = layer
            .track_focus(&focus)
            .key_context("Tour")
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let Some(step) = this.tour.tour.current().cloned() else { return };
                match ev.keystroke.key.as_str() {
                    "enter" if step.task.is_none() || this.tour.tour.task_done || this.tour.waited => {
                        cx.stop_propagation();
                        this.tour_next(window, cx);
                    }
                    "escape" if this.tour.tour.skippable && step.task.is_none() && !this.tour.tour.is_last() => {
                        cx.stop_propagation();
                        this.tour_finish(cx);
                    }
                    _ => {}
                }
            }));
        Some(deferred(layer).with_priority(3).into_any_element())
    }

    /// The card: the bear on the welcome, the title with its ⓘ, one
    /// sentence, the dots and the buttons (`TourCard`).
    fn render_tour_card(&mut self, step: &Step, index: usize, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let fg = style::label_text(cx);
        let secondary = style::secondary_text(cx);
        let done = self.tour.tour.task_done;
        let skippable = self.tour.tour.skippable;
        let is_last = self.tour.tour.is_last();
        let task = step.task;
        let can_next = task.is_none() || done || self.tour.waited;
        let size_cell = self.tour.card_size.clone();
        let mut title_row = h_flex().items_center().gap(px(6.)).child(div().text_size(px(style::size::CARD_TITLE + 1.)).font_semibold().text_color(fg).child(step.title));
        if done {
            title_row = title_row.child(crate::icons::symbol("checkmark.circle.fill").with_size(px(13.)).text_color(secondary));
        }
        title_row = title_row.child(div().flex_1().min_w(px(8.)));
        // By the title, out of the buttons' way: the detail the card leaves
        // out, on the website.
        if let Some(url) = step.doc_url() {
            title_row = title_row.child(
                div()
                    .id("tour-docs").role(Role::Button)
                    .cursor_pointer()
                    .child(crate::icons::symbol("info.circle").with_size(px(14.)).text_color(secondary))
                    .tip("More about this, in the docs")
                    .on_click(move |_, _, cx| {
                        crate::open_gate::open_web(&url, cx);
                    }),
            );
        }
        let body = rules::card_body(step, self.tour_first_failed());
        let mut buttons = h_flex().items_center().gap(px(8.));
        // Where you are in it, as dots, the step on show darker. None for a
        // page's one-card intro: a lone dot says nothing.
        if count > 1 {
            buttons = buttons.child(h_flex().gap(px(5.)).flex_shrink(1.).overflow_hidden().children((0..count).map(|dot| div().flex_none().size(px(5.)).rounded_full().bg(if dot == index { fg } else { fg.opacity(0.2) }))));
        }
        buttons = buttons.child(div().flex_1().min_w(px(8.)));
        if skippable && task != Some(TourTask::SendFirst) && !is_last {
            buttons = buttons.child(
                div()
                    .id("tour-skip").role(Role::Button)
                    .cursor_pointer()
                    .text_color(secondary)
                    .child("Skip")
                    .tip("End the tour (Esc). Help > Take the Tour brings it back")
                    .on_click(cx.listener(|this, _, _, cx| this.tour_finish(cx))),
            );
        }
        if index > 0 {
            buttons = buttons.child(crate::push_button::PushButton::new("tour-back", "Back", "The step before").render(cx, {
                let app = cx.entity().downgrade();
                move |_, window, cx| {
                    app.update(cx, |this, cx| this.tour_back(window, cx)).ok();
                }
            }));
        }
        if task == Some(TourTask::SendFirst) {
            // Sending is the way on; this leaves the message in the composer
            // for later. No Return here: that's the send.
            buttons = buttons.child(crate::push_button::PushButton::new("tour-not-now", "Not now", "End the tour. The message stays in the composer").render(cx, {
                let app = cx.entity().downgrade();
                move |_, _, cx| {
                    app.update(cx, |this, cx| this.tour_finish(cx)).ok();
                }
            }));
        } else {
            let label = if is_last { "Done" } else if index == 0 { "Show me" } else { "Next" };
            let tip = if !can_next { "Have a go first" } else if is_last { "End the tour (Return)" } else { "The next step (Return)" };
            buttons = buttons.child(prominent_button("tour-next", label, tip, can_next, cx).on_click(cx.listener(move |this, _, window, cx| {
                if can_next {
                    this.tour_next(window, cx);
                }
            })));
        }
        let shadow = vec![BoxShadow { color: hsla(0., 0., 0., 0.25), offset: point(gpui_kit::px(0.), gpui_kit::px(6.)), blur_radius: gpui_kit::px(18.), spread_radius: gpui_kit::px(0.), inset: false }];
        v_flex()
            .id("tour-card")
            .relative()
            .w(px(CARD_WIDTH))
            .p(px(16.))
            .gap(px(8.))
            .rounded(gpui_kit::px(12.))
            .bg(cx.theme().background)
            .border(gpui_kit::px(0.5))
            .border_color(style::hairline(cx))
            .shadow(shadow)
            .when(step.bear, |c| c.child(waving_bear(44., secondary)))
            .child(title_row)
            .child(div().text_size(px(style::size::LABEL)).line_height(px(style::natural_line(style::size::LABEL))).text_color(secondary).child(body))
            .child(buttons.pt(px(4.)).text_size(px(style::size::LABEL)))
            // Its size, for placing it beside its part next frame.
            .child(
                canvas(
                    move |b, window, _| {
                        let now = (f32::from(b.size.width), f32::from(b.size.height));
                        if size_cell.get() != now {
                            size_cell.set(now);
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }

    /// The automation's `tour`: {"do": "start"|"next"|"back"|"end"|"pools"},
    /// {"skippable": true} for Help's replay, {"intros": true} to turn the
    /// pages' intros on; says the step, its title and what it lights up, as
    /// the Swift command does: "ok 3/10 Star or put away lit: 52,264 313x51".
    #[cfg(feature = "automation")]
    pub(crate) fn tour_command(&mut self, cmd: &Value, window: &mut Window, cx: &mut Context<Self>) -> Result<String, String> {
        if let Some(on) = cmd["intros"].as_bool() {
            self.tour.intros_off = !on;
        }
        match cmd["do"].as_str() {
            Some("start") => self.tour_start(cmd["skippable"].as_bool().unwrap_or(false), window, cx),
            Some("next") => self.tour_next(window, cx),
            Some("back") => self.tour_back(window, cx),
            Some("end") => self.tour_finish(cx),
            Some("pools") => {
                if rules::pools_intro_due(&self.logins.pools, self.settings.saved.tour_seen == Some(true)) {
                    self.tour_introduce(Intro::Pools, window, cx);
                }
            }
            None => {}
            Some(_) => return Err("do is start, next, back, end or pools".into()),
        }
        Ok(String::new())
    }

    /// What `tour` says once the window has settled.
    #[cfg(feature = "automation")]
    pub(crate) fn tour_line(&self, cx: &App) -> String {
        let Some(index) = self.tour.tour.step else { return "ok off".into() };
        let Some(step) = self.tour.tour.current() else { return "ok off".into() };
        let lit = self.tour_frame(step, cx).map_or_else(|| "nothing".to_owned(), |r| format!("{},{} {}x{}", r.x as i64, r.y as i64, r.w as i64, r.h as i64));
        let task = match step.task {
            None => String::new(),
            Some(_) if self.tour.tour.task_done => " (done)".into(),
            Some(_) => " (to do)".into(),
        };
        format!("ok {}/{} {}{} lit: {}", index + 1, self.tour.tour.shown.len(), step.title, task, lit)
    }

    /// The tour in `dump`: the step, its card and what it lights up.
    pub(crate) fn tour_dump(&self, cx: &App) -> Value {
        let Some(step) = self.tour.tour.current() else { return Value::Null };
        json!({
            "step": self.tour.tour.step.map(|i| i + 1),
            "count": self.tour.tour.shown.len(),
            "title": step.title,
            "body": rules::card_body(step, self.tour_first_failed()),
            "intro": self.tour.tour.intro.map(Intro::name),
            "skippable": self.tour.tour.skippable,
            "taskDone": self.tour.tour.task_done,
            "lit": self.tour_frame(step, cx).map(|r| json!({ "x": r.x, "y": r.y, "width": r.w, "height": r.h })),
            "samples": self.tour.samples.len(),
        })
    }
}

/// The waving bear at `side` points, as the Swift app's `Bear.image(.wave,
/// side:)`: the picture is twice as tall as the bear (room under its feet for
/// the About window's tilt), so only its top half is shown.
pub(crate) fn waving_bear(side: f32, color: Hsla) -> Div {
    div().flex_none().w(px(side)).h(px(side)).overflow_hidden().child(Icon::default().data(BEAR_WAVE).w(px(side)).h(px(side * 2.)).text_color(color))
}

/// The card's default button: the accent, white words, as SwiftUI's
/// `.borderedProminent`; faded and deaf while it can't be pressed.
fn prominent_button(id: &'static str, label: &'static str, tip: &'static str, enabled: bool, cx: &App) -> Stateful<Div> {
    let accent = style::control_accent(cx);
    div()
        .id(id)
        .flex_none()
        .h(px(24.))
        .px(px(12.5))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .whitespace_nowrap()
        .bg(accent)
        .text_color(gpui_kit::white())
        .when(enabled, |b| b.cursor_pointer().active(move |s| s.bg(accent.opacity(0.85))))
        .when(!enabled, |b| b.opacity(0.45))
        .child(label)
        .tip(tip)
}

/// A rounded rectangle, as a subpath of `path`.
fn rounded_rect(path: &mut PathBuilder, r: Rect, corner: f32) {
    let c = corner.min(r.w / 2.).min(r.h / 2.);
    let raw = gpui_kit::px;
    let (x0, y0, x1, y1) = (r.x, r.y, r.max_x(), r.max_y());
    path.move_to(point(raw(x0 + c), raw(y0)));
    path.line_to(point(raw(x1 - c), raw(y0)));
    path.curve_to(point(raw(x1), raw(y0 + c)), point(raw(x1), raw(y0)));
    path.line_to(point(raw(x1), raw(y1 - c)));
    path.curve_to(point(raw(x1 - c), raw(y1)), point(raw(x1), raw(y1)));
    path.line_to(point(raw(x0 + c), raw(y1)));
    path.curve_to(point(raw(x0), raw(y1 - c)), point(raw(x0), raw(y1)));
    path.line_to(point(raw(x0), raw(y0 + c)));
    path.curve_to(point(raw(x0 + c), raw(y0)), point(raw(x0), raw(y0)));
    path.close();
}
