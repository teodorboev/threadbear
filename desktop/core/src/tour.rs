//! The first-run tour, the pages' one-card intros and Finish setting up's
//! checklist, as rules with no window: a port of the Swift app's `Tour`,
//! `TourStep`, `TourIntro` (Views/Tour.swift) and `SetupChecklist`
//! (Views/SetupChecklist.swift). The GPUI app draws them (gpui/src/tour.rs,
//! gpui/src/setup_panel.rs); the steps' words, their order and where the
//! card goes are here, so both apps say the same thing.
//!
//! The tour is short and hands-on: the window dims, each step lights up one
//! part of it with a card beside it, and the last has you send your first
//! thread. It starts by itself the first time the app connects to its runner,
//! with no Skip then, and Help > Take the Tour runs it again, skippable. A few
//! made-up sample threads (`threads.samples`) give it something to point at
//! before anything has run, and go when it ends however it ends (Mike, 29 Oct).
use crate::devices::{Device, Kind};
use crate::inbox::ThreadSummary;
use crate::logins::{Account, Pool};

/// The docs site each card's ⓘ opens a page of.
pub const DOCS_SITE: &str = "https://threadbear.mikecann.app/docs";
/// The tour's last step writes this in the new-thread composer.
pub const FIRST_MESSAGE: &str = "Make me a patch for this folder that adds a button saying hello, then tell me what you did.";
/// Next turns up on a hands-on step after this long, in case the send won't
/// take, so nobody's stuck.
pub const PATIENCE_MS: u64 = 20_000;
/// The settings' names for whether the tour's been seen and which intros
/// have, the same as the Swift app's UserDefaults keys (swift_import.rs).
pub const SEEN_KEY: &str = "tourSeen";
pub const SETUP_HIDDEN_KEY: &str = "setupHidden";

/// The parts of the window the tour points at. Each is measured where it's
/// drawn, so the tour finds it wherever the dock has put it, and skips it
/// when it isn't on screen at all.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Threads,
    Composer,
    /// The chips above the composer, from the thread's patches.
    ActionChips,
    /// The Highlights panel, or its icon on the rail when the panel's closed.
    Highlights,
    HighlightsRail,
    /// The main panel, bar and all.
    MainPanel,
    /// The view icons down the window's left edge.
    ActivityBar,
    /// A sample thread's row in the list, by its title.
    SampleRow(String),
    /// A sample row's hover buttons, while drawn.
    RowButtons,
    /// One of the activity bar's panels ("usage", "updates", "connectors").
    Panel(&'static str),
    /// Finish setting up's panel, opened when the first thread's sent.
    SetupPanel,
}

impl Target {
    /// A panel that may be closed: a step pointing only at these is left out
    /// when none of them is on screen as the tour starts. The rest turn up as
    /// it goes (the samples' rows, the composer, the chips).
    pub fn may_be_closed(&self) -> bool {
        matches!(self, Target::Threads | Target::Highlights | Target::HighlightsRail | Target::ActivityBar)
    }
}

/// The pages and panels with a one-card intro the first time they open, and
/// `Pools`, the first time a pool has a second login to move to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intro {
    Scheduled,
    Projects,
    Librarian,
    Usage,
    Updates,
    Connectors,
    Pools,
}

impl Intro {
    pub const ALL: [Intro; 7] = [Intro::Scheduled, Intro::Projects, Intro::Librarian, Intro::Usage, Intro::Updates, Intro::Connectors, Intro::Pools];

    pub fn name(self) -> &'static str {
        match self {
            Intro::Scheduled => "scheduled",
            Intro::Projects => "projects",
            Intro::Librarian => "librarian",
            Intro::Usage => "usage",
            Intro::Updates => "updates",
            Intro::Connectors => "connectors",
            Intro::Pools => "pools",
        }
    }

    /// Its key in the settings, as the Swift app's (`tourIntro.<name>`).
    pub fn key(self) -> String {
        format!("tourIntro.{}", self.name())
    }

    /// Its one card.
    pub fn card(self) -> Step {
        let (target, title, body, doc) = match self {
            Intro::Scheduled => (Target::MainPanel, "Scheduled", "Schedules send a message at a set time, once or on repeat, to a thread or a new one.", "schedules"),
            Intro::Projects => (Target::MainPanel, "Projects", "Projects group threads by what they're for, with goals and where each is up to.", "projects"),
            Intro::Librarian => (Target::MainPanel, "The librarian", "The librarian uses Haiku on your Claude login to file threads into projects.", "librarian"),
            Intro::Usage => (Target::Panel("usage"), "Usage", "How much room each of your logins has left before it hits its limits.", "accounts"),
            Intro::Updates => (Target::Panel("updates"), "Updates", "What's on its way for the app, the runner and the agents' tools.", "changing-threadbear"),
            Intro::Connectors => (Target::Panel("connectors"), "Connectors", "The services your agents can use, and whether each is signed in.", "connectors"),
            // Nothing in the first run mentions pools: with one login there's
            // nothing for one to do. A second login is when they start to
            // matter, and nothing else would tell you (Mike, 5 Nov).
            Intro::Pools => (Target::Composer, "Pools", "A thread moves to another login in its pool when the one it's on nears its limit.", "accounts"),
        };
        Step { targets: vec![target], doc: Some(doc), ..Step::new(title, body) }
    }
}

/// Which side of what it points at the card goes, in the order they're tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Right,
    Left,
    Below,
    Above,
}

/// Something the tour has them do rather than read about. Moving a panel and
/// pressing a patch's button were hands-on too, until Mike found the tour
/// simpler with them told rather than done (2 Nov).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Send the first thread, its message written for them; sending it moves
    /// on to what's left to set up.
    SendFirst,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// Empty for the welcome, which sits in the middle with nothing lit.
    pub targets: Vec<Target>,
    pub title: &'static str,
    pub body: &'static str,
    pub task: Option<Task>,
    /// What it says instead when the first thread failed (a login that's run
    /// out, say): "while it works" would be wrong.
    pub fail_body: Option<&'static str>,
    pub edges: Vec<Edge>,
    /// The waving bear on the card.
    pub bear: bool,
    /// Opens the sample that needs you first, for a composer, Highlights and
    /// the example patch's chip to point at.
    pub opens_sample: bool,
    /// The sample whose row is drawn hovered, buttons showing, by title.
    pub hovers: Option<&'static str>,
    /// The docs page its ⓘ opens (DOCS_SITE/<doc>), one of
    /// site/app-doc-slugs.json: the detail a one-sentence card leaves out.
    pub doc: Option<&'static str>,
}

impl Step {
    fn new(title: &'static str, body: &'static str) -> Step {
        Step { targets: vec![], title, body, task: None, fail_body: None, edges: vec![Edge::Right, Edge::Left, Edge::Below, Edge::Above], bear: false, opens_sample: false, hovers: None, doc: None }
    }

    /// Its docs page's address, for the ⓘ.
    pub fn doc_url(&self) -> Option<String> {
        self.doc.map(|d| format!("{DOCS_SITE}/{d}"))
    }
}

/// The sample the "Star or put away" step draws hovered.
pub const HOVERED_SAMPLE: &str = "Plan the weekend";

/// One short sentence a card, and a step per idea (Mike, 29 Oct: the first
/// cut's cards were too much to read). The tests hold them to it.
pub fn steps() -> Vec<Step> {
    use Edge::*;
    vec![
        Step { bear: true, ..Step::new("Welcome to Threadbear", "A quick look round, with a few sample threads to try things on.") },
        Step { targets: vec![Target::Threads], doc: Some("threads"), ..Step::new("Threads", "Everything your agents are working on is listed here.") },
        // Shown on a sample's row rather than pressed: pressing them was a
        // chore, and "it comes back by itself" read as a riddle (Mike, 30 Oct).
        Step {
            targets: vec![Target::RowButtons, Target::SampleRow(HOVERED_SAMPLE.into()), Target::Threads],
            hovers: Some(HOVERED_SAMPLE),
            doc: Some("hibernate-and-archive"),
            ..Step::new("Star or put away", "Hover a thread: star it to keep it on top, or put it away with the moon or the box.")
        },
        Step { targets: vec![Target::ActivityBar], doc: Some("threads"), ..Step::new("Your views", "These icons switch to schedules, projects and the librarian.") },
        // No card for Full access / Ask before edits: people know access
        // modes from other agent apps, and the chip says it (Mike, 30 Oct).
        Step { targets: vec![Target::Highlights, Target::HighlightsRail], opens_sample: true, doc: Some("highlights"), ..Step::new("Highlights", "The files, links and questions from a thread collect here.") },
        Step { targets: vec![Target::Highlights, Target::MainPanel], opens_sample: true, doc: Some("panels"), ..Step::new("Move a panel", "Press and hold a panel, then drag it to move it, or onto another to make tabs.") },
        Step { targets: vec![Target::ActionChips], edges: vec![Above, Right, Left, Below], opens_sample: true, doc: Some("patches"), ..Step::new("Patches", "Patches add buttons like these, and panels; ask any thread to make you one.") },
        Step { targets: vec![Target::ActionChips], edges: vec![Above, Right, Left, Below], opens_sample: true, doc: Some("patches"), ..Step::new("Per project or tool", "Each patch belongs to a project, or to a tool wherever you use it.") },
        Step { targets: vec![Target::Composer], task: Some(Task::SendFirst), edges: vec![Above, Below, Right, Left], doc: Some("first-thread"), ..Step::new("Your first thread", "We've written its first message, so press Return to send it.") },
        Step {
            targets: vec![Target::SetupPanel],
            fail_body: Some("Your first thread hit a snag, and its message says how to fix it."),
            doc: Some("finish-setting-up"),
            ..Step::new("Finish setting up", "While it works, pair your phone here to check in from anywhere.")
        },
    ]
}

/// A rectangle in the window, in points from its top left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f32 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f32 {
        self.x + self.w / 2.
    }
    pub fn mid_y(&self) -> f32 {
        self.y + self.h / 2.
    }
    /// Grown by `by` on every side.
    pub fn outset(&self, by: f32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2. * by, self.h + 2. * by)
    }
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect::new(x, y, (self.max_x().min(other.max_x()) - x).max(0.), (self.max_y().min(other.max_y()) - y).max(0.))
    }
}

/// Where the card goes, and the side of the target it went on (None in the
/// middle) for its arrow: beside `target` on the first of `edges` with room,
/// else in the middle of `bounds`. Kept `margin` in from the window's edges.
pub fn card_placement(target: Option<Rect>, card: (f32, f32), bounds: Rect, edges: &[Edge], gap: f32, margin: f32) -> ((f32, f32), Option<Edge>) {
    let (cw, ch) = card;
    let centred = ((bounds.mid_x() - cw / 2., bounds.mid_y() - ch / 2.), None);
    let Some(t) = target else { return centred };
    let clamp_x = |x: f32| x.max(bounds.x + margin).min(bounds.max_x() - margin - cw);
    let clamp_y = |y: f32| y.max(bounds.y + margin).min(bounds.max_y() - margin - ch);
    for &edge in edges {
        match edge {
            Edge::Right if t.max_x() + gap + cw <= bounds.max_x() - margin => return ((t.max_x() + gap, clamp_y(t.mid_y() - ch / 2.)), Some(edge)),
            Edge::Left if t.x - gap - cw >= bounds.x + margin => return ((t.x - gap - cw, clamp_y(t.mid_y() - ch / 2.)), Some(edge)),
            Edge::Below if t.max_y() + gap + ch <= bounds.max_y() - margin => return ((clamp_x(t.mid_x() - cw / 2.), t.max_y() + gap), Some(edge)),
            Edge::Above if t.y - gap - ch >= bounds.y + margin => return ((clamp_x(t.mid_x() - cw / 2.), t.y - gap - ch), Some(edge)),
            _ => continue,
        }
    }
    centred
}

/// Where on the card's edge its arrow sits: level with the target's middle,
/// kept clear of the card's rounded corners (`TourArrow.anchor`).
pub fn arrow_anchor(edge: Edge, card: Rect, target: Rect) -> (f32, f32) {
    let inset = 20.;
    let clamp_y = |y: f32| y.max(card.y + inset).min(card.max_y() - inset);
    let clamp_x = |x: f32| x.max(card.x + inset).min(card.max_x() - inset);
    match edge {
        Edge::Right => (card.x, clamp_y(target.mid_y())),
        Edge::Left => (card.max_x(), clamp_y(target.mid_y())),
        Edge::Below => (clamp_x(target.mid_x()), card.y),
        Edge::Above => (clamp_x(target.mid_x()), card.max_y()),
    }
}

/// What finishing the card on show did, for the app to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finished {
    /// The tour, done or skipped: it doesn't come up by itself again, and the
    /// samples go.
    Tour,
    /// A page's intro: never again.
    Intro(Intro),
}

/// The tour's state, as the Swift `Tour` keeps it.
#[derive(Clone, Debug, Default)]
pub struct Tour {
    /// The step on show, as an index into `shown`; None when it's off.
    pub step: Option<usize>,
    /// The steps this run shows: the ones whose part of the window was on
    /// screen when it started, and the ones whose part turns up as it goes.
    pub shown: Vec<Step>,
    /// False the first time, when it started by itself: no Skip then (Mike,
    /// 29 Oct). Help's replay can be skipped.
    pub skippable: bool,
    /// The step's task is done.
    pub task_done: bool,
    /// The card on show is a page's intro, not the tour.
    pub intro: Option<Intro>,
}

impl Tour {
    pub fn current(&self) -> Option<&Step> {
        self.step.and_then(|i| self.shown.get(i))
    }

    pub fn is_last(&self) -> bool {
        self.step.is_some_and(|i| i + 1 == self.shown.len())
    }

    pub fn is_on(&self) -> bool {
        self.step.is_some()
    }

    /// The first part of `step` that's on screen, by `frame_of`, or None to
    /// put the card in the middle.
    pub fn frame(step: &Step, frame_of: &dyn Fn(&Target) -> Option<Rect>) -> Option<Rect> {
        step.targets.iter().filter_map(frame_of).find(|r| r.w > 1. && r.h > 1.)
    }

    /// Starts the tour on its first step, leaving out the steps that point
    /// only at closed panels.
    pub fn start(&mut self, skippable: bool, frame_of: &dyn Fn(&Target) -> Option<Rect>) {
        self.skippable = skippable;
        self.intro = None;
        self.shown = steps().into_iter().filter(|s| s.task.is_some() || s.targets.is_empty() || s.targets.iter().any(|t| !t.may_be_closed()) || Self::frame(s, frame_of).is_some()).collect();
        self.step = Some(0);
        self.task_done = false;
    }

    /// On to the next step: true if there is one, false when it's the end
    /// (call `finish`).
    pub fn next(&mut self) -> bool {
        match self.step {
            Some(i) if i + 1 < self.shown.len() => {
                self.step = Some(i + 1);
                self.task_done = false;
                true
            }
            _ => false,
        }
    }

    pub fn back(&mut self) {
        if let Some(i) = self.step.filter(|i| *i > 0) {
            self.step = Some(i - 1);
            self.task_done = false;
        }
    }

    /// A page's or panel's intro, unless it's been seen, the tour or another
    /// intro is up, or intros are off.
    pub fn introduce(&mut self, intro: Intro, seen: bool, enabled: bool) -> bool {
        if !enabled || self.step.is_some() || seen {
            return false;
        }
        self.intro = Some(intro);
        self.skippable = true;
        self.shown = vec![intro.card()];
        self.step = Some(0);
        self.task_done = false;
        true
    }

    /// Done or skipped; what the app should remember.
    pub fn finish(&mut self) -> Option<Finished> {
        self.step?;
        self.step = None;
        self.task_done = false;
        Some(match self.intro.take() {
            Some(intro) => Finished::Intro(intro),
            None => Finished::Tour,
        })
    }
}

/// Whether the pools intro is due: once the tour's been seen (the pools come
/// with the runner's hello, just ahead of the tour), when a pool has a second
/// login to move to.
pub fn pools_intro_due(pools: &[Pool], tour_seen: bool) -> bool {
    tour_seen && pools.iter().any(|p| p.account_ids.len() >= 2)
}

/// The words on a card: the fail body once the first thread has failed.
pub fn card_body(step: &Step, first_failed: bool) -> &'static str {
    if first_failed {
        step.fail_body.unwrap_or(step.body)
    } else {
        step.body
    }
}

// ---------------------------------------------------------------- Finish setting up

/// What's left after `./threadbear setup` for a new install. Each item ticks
/// itself off from what the app already knows. Notifications aren't on it
/// (Mike finds them a bit annoying); the phone is pitched for asking and
/// steering while you're out (29 Oct).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupItem {
    FirstThread,
    Phone,
    Website,
}

impl SetupItem {
    pub fn name(self) -> &'static str {
        match self {
            SetupItem::FirstThread => "firstThread",
            SetupItem::Phone => "phone",
            SetupItem::Website => "website",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            SetupItem::FirstThread => "Start your first thread",
            SetupItem::Phone => "Pair your iPhone",
            SetupItem::Website => "Open your website",
        }
    }

    pub fn detail(self, no_login: bool, local_only: bool) -> &'static str {
        match self {
            SetupItem::FirstThread if no_login => "There's no Claude Code or Codex login on this Mac yet. Run claude or codex in Terminal and log in, then come back.",
            SetupItem::FirstThread => "It shows your Claude Code or Codex login works. Try asking what's in the folder.",
            SetupItem::Phone => "Ask questions and steer your threads while you're out, and see how they're getting on.",
            SetupItem::Website if local_only => "Check on your threads from a browser on this Mac, with a link for each.",
            SetupItem::Website => "Check on your threads from any browser, with a link for each.",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetupChecklist {
    /// In order, with whether each is done. What this install can't do (no
    /// Convex mirror, so nothing for a phone to read) is left out rather than
    /// shown stuck.
    pub items: Vec<(SetupItem, bool)>,
    /// No Claude Code or Codex login found: the first thread can't run.
    pub no_login: bool,
}

impl SetupChecklist {
    pub fn left(&self) -> usize {
        self.items.iter().filter(|(_, done)| !done).count()
    }

    pub fn build<'a>(threads: impl IntoIterator<Item = &'a ThreadSummary>, accounts: &[Account], devices: &[Device], can_pair: bool, local_only: bool) -> SetupChecklist {
        // A turn that reached the model (it reported a cost or how full the
        // context is): the login worked. Not the session id, which a thread
        // gets when it's made, the tour's turnless samples too.
        let ran = threads.into_iter().any(|t| (t.cost_usd.is_some() || t.context_tokens.is_some()) && t.status != "error");
        let mut items = vec![(SetupItem::FirstThread, ran)];
        if can_pair {
            let live: Vec<&Device> = devices.iter().filter(|d| !d.expired).collect();
            // A backend on this Mac only can't be reached from a phone.
            if !local_only {
                items.push((SetupItem::Phone, live.iter().any(|d| d.kind == Kind::Phone)));
            }
            items.push((SetupItem::Website, live.iter().any(|d| d.kind == Kind::Browser)));
        }
        SetupChecklist { items, no_login: accounts.is_empty() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Target) -> Option<Rect> {
        None
    }

    #[test]
    fn leaves_out_closed_panels_but_keeps_what_turns_up_as_it_goes() {
        let mut tour = Tour::default();
        // The threads on screen; the activity bar and Highlights not.
        tour.start(false, &|t| (*t == Target::Threads).then(|| Rect::new(44., 0., 300., 800.)));
        let titles: Vec<&str> = tour.shown.iter().map(|s| s.title).collect();
        let all: Vec<&str> = steps().iter().map(|s| s.title).filter(|t| *t != "Your views" && *t != "Highlights").collect();
        assert_eq!(titles, all);
        assert_eq!(tour.step, Some(0));
    }

    #[test]
    fn every_card_is_one_short_sentence() {
        let mut cards = steps();
        cards.extend(Intro::ALL.iter().map(|i| i.card()));
        for step in cards {
            for text in [Some(step.body), step.fail_body].into_iter().flatten() {
                let sentences = text.matches(". ").count();
                assert_eq!(sentences, 0, "one sentence: {text}");
                assert!(text.chars().count() <= 90, "short: {text}");
                assert!(!text.contains('\u{2014}'), "no em dashes: {text}");
            }
            assert!(step.title.chars().count() <= 24, "short title: {}", step.title);
        }
    }

    #[test]
    fn every_docs_link_is_a_page_the_site_has() {
        let slugs: Vec<String> = serde_json::from_str(include_str!("../../../site/app-doc-slugs.json")).unwrap();
        let mut cards = steps();
        cards.extend(Intro::ALL.iter().map(|i| i.card()));
        for step in cards.iter().filter(|s| !s.targets.is_empty()) {
            let doc = step.doc.unwrap_or_else(|| panic!("{} has no docs link", step.title));
            assert!(slugs.iter().any(|s| s == doc), "{doc} isn't in site/app-doc-slugs.json");
        }
    }

    #[test]
    fn the_same_words_as_the_swift_app() {
        // The Swift app's Tour.swift is frozen; these are its steps, in order.
        let swift = include_str!("../../../app/Sources/Threadbear/Views/Tour.swift");
        for step in steps().iter().chain(Intro::ALL.map(|i| i.card()).iter()) {
            assert!(swift.contains(&format!("\"{}\"", step.title)), "Swift has no step called {}", step.title);
            assert!(swift.contains(&format!("\"{}\"", step.body)), "Swift says something else for {}", step.title);
        }
        assert!(swift.contains(FIRST_MESSAGE));
    }

    #[test]
    fn an_intro_shows_once_and_not_when_off() {
        let mut tour = Tour::default();
        assert!(tour.introduce(Intro::Projects, false, true));
        assert_eq!(tour.current().map(|s| s.title), Some("Projects"));
        assert!(tour.is_last(), "one card");
        assert_eq!(tour.finish(), Some(Finished::Intro(Intro::Projects)));
        assert!(!tour.introduce(Intro::Projects, true, true), "only the first time");
        assert!(!tour.introduce(Intro::Usage, false, false), "not while they're off");
        tour.start(false, &none);
        assert!(!tour.introduce(Intro::Usage, false, true), "not over the tour");
    }

    #[test]
    fn pools_intro_waits_for_a_second_login_and_the_tour() {
        let pool = |n: usize| Pool { id: "claude".into(), title: "Claude".into(), account_ids: (0..n).map(|i| i.to_string()).collect(), icon: None, folders: vec![] };
        assert!(!pools_intro_due(&[pool(2)], false), "not before the tour");
        assert!(!pools_intro_due(&[pool(1), pool(1)], true), "a Claude and a Codex login are two pools of one");
        assert!(pools_intro_due(&[pool(2)], true));
    }

    #[test]
    fn only_the_replay_can_be_skipped() {
        let mut tour = Tour::default();
        tour.start(false, &none);
        assert!(!tour.skippable);
        tour.start(true, &none);
        assert!(tour.skippable);
    }

    #[test]
    fn highlights_falls_back_to_the_rail_icon() {
        let rail = Rect::new(1140., 40., 32., 32.);
        let step = steps().into_iter().find(|s| s.title == "Highlights").unwrap();
        assert_eq!(Tour::frame(&step, &|t| (*t == Target::HighlightsRail).then_some(rail)), Some(rail));
    }

    #[test]
    fn next_on_the_last_step_ends_it() {
        let mut tour = Tour::default();
        tour.start(true, &none);
        while tour.next() {}
        assert!(tour.is_last());
        assert_eq!(tour.finish(), Some(Finished::Tour));
        assert!(!tour.is_on());
        assert_eq!(tour.finish(), None);
    }

    #[test]
    fn card_goes_beside_its_target_where_there_is_room() {
        let bounds = Rect::new(0., 0., 1200., 800.);
        let card = (300., 150.);
        let at = |t: Option<Rect>, edges: &[Edge]| card_placement(t, card, bounds, edges, 14., 12.).0;
        assert_eq!(at(Some(Rect::new(44., 0., 300., 800.)), &[Edge::Right, Edge::Left]), (358., 325.));
        assert_eq!(at(Some(Rect::new(900., 0., 300., 800.)), &[Edge::Right, Edge::Left]), (586., 325.));
        assert_eq!(at(Some(Rect::new(900., 700., 290., 80.)), &[Edge::Above]), (888., 536.));
        assert_eq!(at(Some(bounds), &[Edge::Right, Edge::Left, Edge::Below, Edge::Above]), (450., 325.));
        assert_eq!(at(None, &[Edge::Right]), (450., 325.));
    }

    fn thread(ran: bool, status: &str) -> ThreadSummary {
        let mut t: ThreadSummary = serde_json::from_value(serde_json::json!({ "id": "t", "title": "", "cwd": "/tmp", "status": status })).unwrap();
        t.cost_usd = ran.then_some(0.01);
        t
    }

    fn account() -> Account {
        serde_json::from_value(serde_json::json!({ "id": "a", "title": "a" })).unwrap()
    }

    fn device(kind: &str, expired: bool) -> Device {
        Device::decode(&serde_json::json!({ "id": kind, "kind": kind, "name": kind, "expired": expired })).unwrap()
    }

    #[test]
    fn a_new_install_has_everything_left() {
        let list = SetupChecklist::build([], &[account()], &[], true, false);
        assert_eq!(list.items.iter().map(|i| i.0).collect::<Vec<_>>(), vec![SetupItem::FirstThread, SetupItem::Phone, SetupItem::Website]);
        assert_eq!(list.left(), 3);
        assert!(!list.no_login);
    }

    #[test]
    fn a_backend_on_this_mac_only_leaves_the_phone_off() {
        let list = SetupChecklist::build([], &[account()], &[], true, true);
        assert_eq!(list.items.iter().map(|i| i.0).collect::<Vec<_>>(), vec![SetupItem::FirstThread, SetupItem::Website]);
    }

    #[test]
    fn each_ticks_itself_off() {
        let t = thread(true, "idle");
        let list = SetupChecklist::build([&t], &[account()], &[device("phone", false), device("browser", false)], true, false);
        assert_eq!(list.left(), 0);
    }

    #[test]
    fn a_failed_or_turnless_thread_doesnt_count() {
        for t in [thread(false, "idle"), thread(true, "error")] {
            let list = SetupChecklist::build([&t], &[account()], &[], false, false);
            assert!(!list.items[0].1);
        }
    }

    #[test]
    fn an_expired_phone_needs_pairing_again() {
        let list = SetupChecklist::build([], &[account()], &[device("phone", true)], true, false);
        assert_eq!(list.items.iter().find(|i| i.0 == SetupItem::Phone).map(|i| i.1), Some(false));
    }

    #[test]
    fn without_the_mirror_the_phone_and_website_arent_listed() {
        let list = SetupChecklist::build([], &[], &[], false, false);
        assert_eq!(list.items.iter().map(|i| i.0).collect::<Vec<_>>(), vec![SetupItem::FirstThread]);
        assert!(list.no_login);
    }
}
