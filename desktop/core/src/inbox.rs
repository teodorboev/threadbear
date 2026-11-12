// The sidebar's sections: the rules of the Mac's ThreadSections
// (app/Sources/ThreadbearCore/Threads/ThreadSections.swift), which the phone's
// InboxSections and the website's web/src/sections.ts follow too. A summary
// is what `threads.list` gives; what a row says about it is in `sidebar.rs`.
use std::collections::HashMap;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub provider: String,
    pub model: Option<String>,
    pub model_in_use: Option<String>,
    pub account_id: String,
    /// The pool that picks its login; none when it's pinned to `account_id`.
    pub pool_id: Option<String>,
    pub effort: Option<String>,
    pub effort_in_use: Option<String>,
    pub permission_mode: String,
    pub context_tokens: Option<i64>,
    pub context_window: Option<i64>,
    pub cost_usd: Option<f64>,
    pub status: String,
    pub needs_you: bool,
    pub needs: Option<String>,
    pub asks: Option<Vec<OpenAsk>>,
    pub asks_seq: Option<u64>,
    pub pending_reviews: Option<u64>,
    pub last_activity_at: String,
    pub snoozed_until: Option<String>,
    pub snoozed_until_restart: Option<bool>,
    pub snoozed_until_release: Option<bool>,
    pub wake_when: Option<Value>,
    pub waiting_on: Option<Vec<Value>>,
    pub woke: Option<Value>,
    pub starred_at: Option<String>,
    pub archived_at: Option<String>,
    pub waiting_for_room: Option<Value>,
    /// Whether a turn that hits its login's limit waits for room and carries
    /// on by itself: on unless Mike turned it off on the limit box, when the
    /// runner sends `false` (and nothing otherwise).
    #[serde(default = "carry_on_by_default")]
    pub carry_on_at_reset: bool,
    pub waiting_for_network: Option<Value>,
    /// Kept as it came: the reducer reads its own `BackgroundTask` from these.
    pub background: Option<Vec<Value>>,
    pub subagents: Option<Vec<Value>>,
    /// The runner's loose-ends check for the done card (protocol "Loose
    /// ends"), kept as it came and read with `loose_ends_check`, so one
    /// the reducer can't read leaves the rest of the summary standing.
    pub loose_ends: Option<Value>,
    pub projects: Option<Vec<Project>>,
    pub started_by: Option<Value>,
    pub schedule: Option<Value>,
    pub moving: Option<Value>,
    pub asked: Option<String>,
    pub preview: String,
    /// The librarian's search words, for finding a thread by what it's about
    /// when its title and preview don't say it (`matches`).
    pub keywords: Option<String>,
    pub last_seq: u64,
    pub last_seen_seq: u64,
    /// The rail's panel buttons (`ThreadSummary.panels`) and Patches, as they
    /// came: the window shell draws a button for each. Kept as they came and
    /// decoded leniently on demand (highlights.rs), as `background` and
    /// `subagents` are.
    pub panels: Option<Vec<Value>>,
    pub actions: Option<Vec<Value>>,
    pub patches: Option<Vec<Value>>,
    pub hooks: Option<Vec<Value>>,
    /// Moves when a pin, a removal or a brief changes what `threads.highlights`
    /// would answer, without a new event.
    pub highlights_at: Option<String>,
    /// The thread's worktree, while it has one (protocol "Worktrees").
    pub worktree: Option<Value>,
    pub on_old_runner: Option<bool>,
    /// Sharing (protocol "Sharing"): `{ access, slug }`.
    pub share: Option<Value>,
    /// When it was made (the About window's numbers, den.rs).
    pub created_at: Option<String>,
    /// One of the tour's made-up threads (`threads.samples`): left out of
    /// the About window's numbers.
    pub sample: Option<bool>,
    /// Which other runner it lives on; none for this Mac's own threads.
    /// Never on the wire: a runner's summaries are all its own (Swift
    /// `ThreadSummary.runnerId`), and `remote::summary` sets it.
    #[serde(skip)]
    pub runner_id: Option<String>,
    /// A panel the agent asked to show Mike, waiting for him to look at the
    /// thread (panel_requests.rs).
    pub panel_request: Option<crate::panel_requests::PanelRequest>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenAsk {
    pub action: String,
    #[serde(rename = "where")]
    pub place: Option<String>,
    pub why: Option<String>,
    pub options: Option<Vec<String>>,
    pub suggested: Option<usize>,
    pub ticked: Option<bool>,
    /// What each option does, the reply a plain ask wants, and the ask's
    /// links, as they came: a decision on a project's page draws the thread's
    /// card from these (`projects::decision_asks`), as its composer draws it
    /// from the turn.
    pub option_notes: Option<Vec<String>>,
    pub reply: Option<String>,
    pub links: Option<Vec<Value>>,
}

impl OpenAsk {
    /// Where it's to be done, in plain words (`plainWhere`).
    pub fn where_text(&self) -> Option<String> {
        self.place.as_deref().filter(|w| !w.is_empty()).map(crate::sidebar::preview_plain)
    }
}

fn carry_on_by_default() -> bool {
    true
}

/// One of a thread's projects (`ThreadProject`): who put it there, and the
/// librarian's reason, behind the project's ⓘ in Highlights.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub title: String,
    /// "mike", "agent" or "librarian".
    pub by: String,
    pub pinned: bool,
    pub why: Option<String>,
}

impl ThreadSummary {
    /// While it's on its way to another machine, what its box says in place
    /// of the words ("Moving to bruce…"): anything typed or answered now
    /// could land on the wrong side of the move, so the box and the card
    /// wait. None once it's there, or once the move failed or was called off
    /// (`ThreadMove.line`, ComposerView's `moving`).
    pub fn moving_line(&self) -> Option<String> {
        let moving = self.moving.as_ref()?;
        let state = moving.get("state").and_then(Value::as_str).unwrap_or("");
        let name = moving.get("toName").and_then(Value::as_str).unwrap_or("");
        match state {
            "failed" | "cancelled" => None,
            "ready" => Some(format!("Moving to {name} once this turn ends…")),
            _ => Some(format!("Moving to {name}…")),
        }
    }

    /// Newer events than Mike has seen.
    pub fn unread(&self) -> bool {
        self.last_seq > self.last_seen_seq
    }
    /// A turn is running or starting (ThreadStatus.isBusy).
    /// A turn is underway or about to be (Swift `ThreadStatus.isBusy`),
    /// waiting on a permission card included: that turn can still be stopped,
    /// and the composer offered Send instead of Stop for one (10 Nov).
    pub fn busy(&self) -> bool {
        matches!(self.status.as_str(), "running" | "starting" | "waiting")
    }
    /// The background list, read leniently: a malformed entry is left out.
    pub fn background_tasks(&self) -> Vec<BackgroundTask> {
        self.background.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
    }
    /// What's still open on the done card, as the reducer takes it
    /// (Swift `ThreadSummary.looseEnds`, which decodes it leniently too).
    pub fn loose_ends_check(&self) -> Option<crate::reducer::LooseEnds> {
        self.loose_ends.as_ref().and_then(crate::reducer::LooseEnds::decode)
    }
    /// Sub-agents or a workflow still at work, or just finished with their
    /// result still to be read (Swift `agentsStillWorking`).
    pub fn agents_still_working(&self) -> bool {
        self.background.iter().flatten().any(|b| matches!(b.get("kind").and_then(Value::as_str), Some("agent") | Some("other")))
    }
    /// The first name for `agents_still_working`, kept for the thread pane.
    pub fn working(&self) -> bool {
        self.agents_still_working()
    }
    /// Background work and no turn to show for it (Swift
    /// `isWorkingInBackground`): any kind of job counts here, a shell one too.
    pub fn working_in_background(&self) -> bool {
        self.background.as_ref().is_some_and(|b| !b.is_empty()) && !self.busy()
    }
    pub fn said_done(&self) -> bool {
        self.needs.as_deref() == Some("nothing")
    }
    /// A design review waiting on Mike, once the thread has finished working
    /// (Swift `reviewWaiting`).
    pub fn review_waiting(&self) -> bool {
        self.pending_reviews.unwrap_or(0) > 0 && !self.busy() && self.background.as_ref().is_none_or(|b| b.is_empty())
    }
    fn needs_answer(&self) -> bool {
        self.status == "waiting"
    }
    /// The last turn ended asking Mike for something and he hasn't done it
    /// all yet (Swift `waitingOnMike`, whose comments say why sub-agents
    /// still working count differently from a shell job).
    pub fn waiting_on_mike(&self) -> bool {
        if self.needs.as_deref() != Some("you") || self.busy() || self.agents_still_working() {
            return false;
        }
        let asks = self.asks.as_deref().unwrap_or(&[]);
        asks.is_empty() || asks.iter().any(|a| a.ticked != Some(true))
    }
    /// Snoozed past `now_ms`. A restart snooze carries a far-off `snoozedUntil` too.
    pub fn snoozed(&self, now_ms: i64) -> bool {
        self.snoozed_until_restart == Some(true) || self.snoozed_until_ms().is_some_and(|t| t > now_ms)
    }
    pub fn snoozed_until_ms(&self) -> Option<i64> {
        self.snoozed_until.as_deref().and_then(parse_ms)
    }
    /// How much an inbox thread wants Mike, lowest first: InboxUrgency, whose
    /// cases are shared with the phone and the website (and tested below).
    pub fn urgency(&self) -> u8 {
        let broken = self.status == "error" || self.status == "stopped";
        if self.review_waiting() || self.needs_answer() {
            0
        } else if self.needs_you {
            if broken { 1 } else { 2 }
        } else if broken {
            3
        } else {
            4
        }
    }
    /// Wants Mike (InboxUrgency.counts): what the Dock badge would count.
    pub fn wants_you(&self) -> bool {
        self.urgency() <= 2
    }
    /// The line under a row's title: its projects, or its folder (`ProjectMark.place`).
    pub fn place(&self) -> String {
        match self.projects.as_deref() {
            Some(projects) if !projects.is_empty() => projects.iter().map(|p| if p.title.is_empty() { "Untitled project" } else { p.title.as_str() }).collect::<Vec<_>>().join(", "),
            _ => folder_name(&self.cwd),
        }
    }
    pub fn activity_ms(&self) -> i64 {
        parse_ms(&self.last_activity_at).unwrap_or(0)
    }
    pub fn archived_ms(&self) -> Option<i64> {
        self.archived_at.as_deref().and_then(parse_ms)
    }
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
    pub fn is_starred(&self) -> bool {
        self.starred_at.is_some()
    }
    pub fn waiting_for_room_until_ms(&self) -> Option<i64> {
        self.waiting_for_room.as_ref()?.get("until")?.as_str().and_then(parse_ms)
    }
    /// The thread's panels (a page, a picture or a list), read leniently.
    pub fn panels(&self) -> Vec<crate::highlights::Panel> {
        self.panels.iter().flatten().filter_map(crate::highlights::Panel::from_value).collect()
    }
    /// Its action chips, read leniently.
    pub fn actions(&self) -> Vec<crate::highlights::Action> {
        self.actions.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
    }
    /// The patches that apply to it, read leniently.
    pub fn patches(&self) -> Vec<crate::highlights::PatchInfo> {
        self.patches.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
    }
    pub fn hooks(&self) -> Vec<crate::highlights::HookInfo> {
        self.hooks.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
    }
    /// A thread on another runner, shown here (`isRemote`).
    pub fn is_remote(&self) -> bool {
        self.runner_id.is_some()
    }
}

/// One item of `ThreadSummary.background` (Swift `BackgroundTask`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BackgroundTask {
    /// "agent", "shell", "monitor" or "other"; anything else reads as "other".
    pub kind: String,
    pub description: String,
    pub finished: bool,
}

/// How the Inbox section is ordered, from the sort menu on its heading
/// (Swift `InboxOrder`). Newest and oldest ignore urgency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InboxOrder {
    #[default]
    NeedsYou,
    Newest,
    Oldest,
}

impl InboxOrder {
    pub const ALL: [InboxOrder; 3] = [InboxOrder::NeedsYou, InboxOrder::Newest, InboxOrder::Oldest];

    pub fn title(self) -> &'static str {
        match self {
            InboxOrder::NeedsYou => "Needs you first",
            InboxOrder::Newest => "Newest first",
            InboxOrder::Oldest => "Oldest first",
        }
    }

    /// The name scripts and the Swift app use for it.
    pub fn name(self) -> &'static str {
        match self {
            InboxOrder::NeedsYou => "needsYou",
            InboxOrder::Newest => "newest",
            InboxOrder::Oldest => "oldest",
        }
    }

    pub fn from_name(name: &str) -> Option<InboxOrder> {
        InboxOrder::ALL.into_iter().find(|o| o.name() == name)
    }

    /// Its icon in the sort menu.
    pub fn symbol(self) -> &'static str {
        match self {
            InboxOrder::NeedsYou => "hand.raised",
            InboxOrder::Newest => "arrow.down",
            InboxOrder::Oldest => "arrow.up",
        }
    }
}

pub struct Section {
    #[allow(dead_code)] // the tests find sections by it
    pub key: &'static str,
    pub title: &'static str,
    pub rows: Vec<ThreadSummary>,
}

/// Sections in the Mac app's default order (`ThreadSections.build`), with no
/// schedules known.
pub fn sections<'a>(threads: impl IntoIterator<Item = &'a ThreadSummary>, now_ms: i64) -> Vec<Section> {
    sections_ordered(threads, now_ms, InboxOrder::NeedsYou, &HashMap::new())
}

/// When a schedule next messages a thread by itself, and its words for when
/// ("Weekdays at 9:00").
#[derive(Clone, Debug, PartialEq)]
pub struct Wake {
    pub at_ms: i64,
    pub when_text: String,
}

/// `ThreadSections.wakes`: for each thread a schedule will message by itself,
/// the one due soonest. Paused and finished ones don't count, nor ones that
/// start a new thread.
pub fn wakes(schedules: &[crate::highlights::Schedule]) -> HashMap<String, Wake> {
    let mut soonest: HashMap<String, Wake> = HashMap::new();
    for schedule in schedules.iter().filter(|s| s.is_active()) {
        let (Some(thread), Some(at_ms)) = (schedule.thread_id.as_ref(), schedule.next_run_at.as_deref().and_then(parse_ms)) else { continue };
        if soonest.get(thread).is_some_and(|w| w.at_ms <= at_ms) {
            continue;
        }
        soonest.insert(thread.clone(), Wake { at_ms, when_text: schedule.when_text.clone() });
    }
    soonest
}

/// `ThreadSections.needsYouCount`: the threads waiting on Mike, for the Dock
/// badge, without sorting anything: one in the Inbox, or starred and awake,
/// that wants him (`wants_you`).
pub fn needs_you_count<'a>(threads: impl IntoIterator<Item = &'a ThreadSummary>, now_ms: i64) -> usize {
    threads
        .into_iter()
        .filter(|t| {
            if !t.wants_you() || t.is_archived() {
                return false;
            }
            if t.is_starred() {
                return t.snoozed_until_ms().is_none();
            }
            if t.snoozed(now_ms) {
                return false;
            }
            // In the Inbox: what needs Mike goes there even with work going in
            // the background; otherwise a running thread is under Running.
            t.review_waiting() || t.needs_answer() || t.waiting_on_mike() || (t.needs_you && !t.working_in_background()) || !(t.busy() || t.working_in_background())
        })
        .count()
}

/// The Dock badge's words: the count, or none (`DockBadge.label`).
pub fn dock_badge(count: usize) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

/// What a notification for a thread that's come to need Mike says
/// (`Notifier.post`): its title, and what it asked for, else its last words.
pub fn notification_words(t: &ThreadSummary) -> (String, String) {
    let title = if t.title.is_empty() { "Threadbear".to_owned() } else { t.title.clone() };
    let body = if t.status == "waiting" {
        "Waiting for you to answer.".to_owned()
    } else if let Some(action) = t.asks.as_ref().and_then(|asks| asks.iter().find(|a| a.ticked != Some(true))).map(|a| crate::sidebar::preview_plain(&a.action)) {
        action
    } else if t.preview.is_empty() {
        "Finished.".to_owned()
    } else {
        crate::sidebar::preview_plain(&t.preview)
    };
    (title, body)
}

/// `AppModel.apply`'s rule for a notification: the thread has just come to
/// need Mike (it didn't before, or wasn't known) and he isn't looking at it.
/// Never another runner's thread: notifications are low-key, and that Mac's
/// app says so there.
pub fn should_notify(before: Option<&ThreadSummary>, after: &ThreadSummary, looking_at_it: bool) -> bool {
    after.needs_you && !before.is_some_and(|b| b.needs_you) && !looking_at_it && after.runner_id.is_none()
}

/// The notification's identifier, as the Swift app's (`Notifier.post`): one
/// per thread and point in it, so a later one doesn't replace an earlier one
/// still unread, and the thread comes back out of it when it's clicked.
pub fn notification_tag(t: &ThreadSummary) -> String {
    format!("{}-{}", t.id, t.last_seq)
}

/// The thread a notification's tag names (`notification_tag`).
pub fn thread_in_tag(tag: &str) -> Option<&str> {
    tag.rsplit_once('-').map(|(id, _)| id).filter(|id| !id.is_empty())
}

/// `ThreadSections.build`: where each thread goes and the order within each
/// heading. `wakes` are the threads a schedule is due to message
/// (`inbox::wakes`): one that's idle with nothing for Mike waits for it under
/// Hibernating rather than sitting in the Inbox.
pub fn sections_ordered<'a>(threads: impl IntoIterator<Item = &'a ThreadSummary>, now_ms: i64, order: InboxOrder, wakes: &HashMap<String, Wake>) -> Vec<Section> {
    let (mut starred, mut inbox, mut running, mut hibernating, mut archived) = (vec![], vec![], vec![], vec![], vec![]);
    for t in threads {
        if t.is_archived() {
            archived.push(t.clone());
        } else if t.is_starred() {
            starred.push(t.clone());
        } else if t.snoozed(now_ms) {
            hibernating.push(t.clone());
        } else if t.review_waiting() || t.needs_answer() || t.waiting_on_mike() || (t.needs_you && !t.working_in_background()) {
            inbox.push(t.clone());
        } else if t.busy() || t.working_in_background() {
            running.push(t.clone());
        } else if t.status == "idle" && t.needs.as_deref() != Some("you") && t.waiting_for_room.is_none() && wakes.contains_key(&t.id) {
            hibernating.push(t.clone());
        } else {
            inbox.push(t.clone());
        }
    }
    // Sorted by keys worked out once per thread: a time is text to be read,
    // and comparing two threads by reading theirs again each time cost the
    // app more than drawing a frame does (the activity bar's badge counts the
    // Inbox with this on every frame).
    use std::cmp::Reverse;
    let newest_first = |t: &ThreadSummary| (Reverse(t.activity_ms()), t.id.clone());
    starred.sort_by(|a, b| a.starred_at.cmp(&b.starred_at).then_with(|| a.id.cmp(&b.id)));
    match order {
        InboxOrder::NeedsYou => inbox.sort_by_cached_key(|t| (t.urgency(), newest_first(t))),
        InboxOrder::Newest => inbox.sort_by_cached_key(newest_first),
        InboxOrder::Oldest => inbox.sort_by_cached_key(|t| (t.activity_ms(), Reverse(t.id.clone()))),
    }
    running.sort_by_cached_key(newest_first);
    // A snooze's time, or for one waiting on a schedule its next run. Back on
    // restart has no time, so it goes last.
    let wakes_at = |t: &ThreadSummary| t.snoozed_until_ms().filter(|_| t.snoozed_until_restart != Some(true)).or_else(|| wakes.get(&t.id).map(|w| w.at_ms)).unwrap_or(i64::MAX);
    hibernating.sort_by_cached_key(|t| (wakes_at(t), newest_first(t)));
    archived.sort_by_cached_key(|t| (Reverse(t.archived_ms()), t.id.clone()));
    [("starred", "Starred", starred), ("inbox", "Inbox", inbox), ("running", "Running", running), ("hibernating", "Hibernating", hibernating), ("archived", "Archived", archived)]
        .into_iter()
        .filter(|(key, _, rows)| !rows.is_empty() || *key == "inbox")
        .map(|(key, title, rows)| Section { key, title, rows })
        .collect()
}

/// Whether a sidebar search finds this thread: its title or its last reply,
/// as ThreadSections.matches has it. `query` is already lowercase.
/// Title, preview or the librarian's search words, case and accent
/// insensitive (`ThreadSections.matches`). The search words find a thread by
/// what it's about when neither says it ("dentist" for a wisdom tooth
/// appointment).
pub fn matches(t: &ThreadSummary, query: &str) -> bool {
    let query = fold(query);
    [Some(t.title.as_str()), Some(t.preview.as_str()), t.keywords.as_deref()].into_iter().flatten().any(|text| fold(text).contains(&query))
}

/// Lower case with the accents taken off, for comparing as Swift's
/// `[.caseInsensitive, .diacriticInsensitive]` does: "Café" and "cafe" are
/// the same. Combining marks go, and the accented Latin letters become their
/// plain ones; anything else is only lower-cased.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        match plain_letter(c) {
            Some(plain) => out.push(plain),
            None => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn plain_letter(c: char) -> Option<char> {
    Some(match c {
        'À'..='Å' | 'à'..='å' | 'Ā'..='ą' => 'a',
        'Ç' | 'ç' | 'Ć'..='č' => 'c',
        'Ď'..='đ' => 'd',
        'È'..='Ë' | 'è'..='ë' | 'Ē'..='ě' => 'e',
        'Ĝ'..='ģ' => 'g',
        'Ĥ'..='ħ' => 'h',
        'Ì'..='Ï' | 'ì'..='ï' | 'Ĩ'..='ı' => 'i',
        'Ĵ' | 'ĵ' => 'j',
        'Ķ' | 'ķ' => 'k',
        'Ĺ'..='ł' => 'l',
        'Ñ' | 'ñ' | 'Ń'..='ň' => 'n',
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' | 'Ō'..='ő' => 'o',
        'Ŕ'..='ř' => 'r',
        'Ś'..='š' => 's',
        'Ţ'..='ŧ' => 't',
        'Ù'..='Ü' | 'ù'..='ü' | 'Ũ'..='ų' => 'u',
        'Ŵ' | 'ŵ' => 'w',
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' | 'Ÿ' => 'y',
        'Ź'..='ž' => 'z',
        _ => return None,
    })
}

/// The last part of a folder's path, on any platform.
pub fn folder_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().filter(|s| !s.is_empty()).unwrap_or(path).to_string()
}

/// "claude-opus-5-5" → "Claude Opus 5.5", "opus" → "Claude Opus",
/// "gpt-6-astra" → "GPT-6 Astra", and with no model the CLI's name: a port
/// of web/src/format.ts's `modelLabel`.
pub fn model_label(provider: &str, model: Option<&str>) -> String {
    let cli = if provider == "codex" { "Codex" } else { "Claude" };
    let Some(model) = model.filter(|m| !m.is_empty()) else { return cli.into() };
    let id = match model.find('[') {
        Some(at) if model.ends_with(']') => &model[..at],
        _ => model,
    };
    let cap = |w: &str| w.chars().next().map(|c| c.to_uppercase().collect::<String>() + &w[c.len_utf8()..]).unwrap_or_default();
    let lower = id.to_lowercase();
    let rest = lower.strip_prefix("claude-").unwrap_or(&lower);
    for family in ["opus", "sonnet", "haiku", "fable"] {
        if let Some(after) = rest.strip_prefix(family) {
            let nums: Vec<&str> = after.split('-').skip(1).take(2).take_while(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).collect();
            if !after.is_empty() && !after.starts_with('-') {
                break;
            }
            let version = if nums.is_empty() { String::new() } else { format!(" {}", nums.join(".")) };
            return format!("Claude {}{version}", cap(family));
        }
    }
    if let Some(after) = lower.strip_prefix("gpt-") {
        let mut parts = after.splitn(2, '-');
        let version = parts.next().unwrap_or("");
        if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            let name = parts.next().map(|n| format!(" {}", n.split('-').map(cap).collect::<Vec<_>>().join(" "))).unwrap_or_default();
            return format!("GPT-{version}{name}");
        }
    }
    id.to_string()
}

/// "now", "5m", "3h", "2d", then the date, as web/src/format.ts's `ago`.
pub fn ago(ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - ms) / 1000).max(0);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ if s < 86400 * 7 => format!("{}d", s / 86400),
        _ => format!("{}w", s / (86400 * 7)),
    }
}

/// Milliseconds since 1970 to ISO 8601 in UTC, as the protocol writes times.
pub fn iso_time(ms: i64) -> String {
    // Howard Hinnant's civil_from_days, the inverse of what parse_ms uses.
    let (secs, millis) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let days = secs.div_euclid(86400);
    let rest = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// ISO 8601 in UTC (the protocol's times) to milliseconds since 1970.
pub fn parse_ms(iso: &str) -> Option<i64> {
    let b = iso.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| iso.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = if b.get(19) == Some(&b'.') {
        let digits: String = iso[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
    } else {
        0
    };
    // Days from 1970-01-01 (Howard Hinnant's days_from_civil).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + s * 1000 + frac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_thread_on_its_way_to_another_machine_says_so_in_its_box() {
        let moving = |state: &str| thread(1, json!({ "moving": { "moveId": "m1", "to": "r2", "toName": "bruce", "state": state } }));
        assert_eq!(moving("copying").moving_line().as_deref(), Some("Moving to bruce…"));
        assert_eq!(moving("ready").moving_line().as_deref(), Some("Moving to bruce once this turn ends…"));
        assert_eq!(moving("failed").moving_line(), None, "a failed move gives the box back");
        assert_eq!(moving("cancelled").moving_line(), None);
        assert_eq!(thread(2, json!({})).moving_line(), None);
    }

    fn thread(n: u32, over: Value) -> ThreadSummary {
        let mut v = json!({
            "id": format!("t{n}"), "title": format!("Thread {n}"), "cwd": "/home/mike/dev/x",
            "provider": "claude", "status": "idle", "needsYou": false,
            "lastActivityAt": format!("2026-11-06T10:{n:02}:00Z"), "preview": "", "lastSeq": 10, "lastSeenSeq": 10
        });
        v.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }
    fn ids(s: &[Section], key: &str) -> Vec<String> {
        s.iter().find(|x| x.key == key).map(|x| x.rows.iter().map(|r| r.id.clone()).collect()).unwrap_or_default()
    }
    fn now() -> i64 {
        parse_ms("2026-11-06T12:00:00Z").unwrap()
    }

    #[test]
    fn the_badge_counts_what_waits_on_mike_and_a_notification_says_what_it_asked() {
        let asks = thread(1, json!({ "needs": "you", "needsYou": true, "asks": [{ "action": "Ship **it**?" }] }));
        let unread = thread(2, json!({ "needsYou": true, "lastSeenSeq": 3 }));
        let read = thread(3, json!({}));
        let asleep = thread(4, json!({ "needsYou": true, "snoozedUntil": "2026-11-07T00:00:00Z" }));
        let all = [asks.clone(), unread, read, asleep];
        assert_eq!(needs_you_count(&all, now()), 2);
        assert_eq!(dock_badge(2).as_deref(), Some("2"));
        assert_eq!(dock_badge(0), None);
        let (title, body) = notification_words(&asks);
        assert_eq!(title, "Thread 1");
        assert_eq!(body, "Ship it?");
    }

    #[test]
    fn search_finds_the_librarians_words_whatever_the_case_and_accents() {
        let t = thread(1, json!({ "title": "Café plans", "preview": "Booked the RÉSUMÉ review", "keywords": "dentist wisdom tooth" }));
        assert!(matches(&t, "cafe"), "an accent in the title");
        assert!(matches(&t, "CAFÉ"));
        assert!(matches(&t, "resume"), "and in the preview");
        assert!(matches(&t, "Dentist"), "the librarian's words");
        assert!(!matches(&t, "dentistry"));
        assert_eq!(fold("Ångström Łódź Ñandú"), "angstrom lodz nandu");
        assert_eq!(fold("e\u{301}"), "e", "a combining accent goes too");
    }

    #[test]
    fn a_notification_comes_once_when_a_thread_starts_needing_mike_elsewhere() {
        let quiet = thread(1, json!({}));
        let needy = thread(1, json!({ "needsYou": true, "lastSeq": 42 }));
        assert!(should_notify(Some(&quiet), &needy, false));
        assert!(should_notify(None, &needy, false), "a thread first heard of needing him");
        assert!(!should_notify(Some(&needy), &needy, false), "only when it starts");
        assert!(!should_notify(Some(&quiet), &needy, true), "not while he's looking at it");
        let mut remote = needy.clone();
        remote.runner_id = Some("other-mac".into());
        assert!(!should_notify(Some(&quiet), &remote, false), "another runner's says so on that Mac");
        let tag = notification_tag(&needy);
        assert_eq!(tag, "t1-42");
        assert_eq!(thread_in_tag(&tag), Some("t1"));
        assert_eq!(thread_in_tag("b0e6c5a2-1111-2222-3333-444455556666-7"), Some("b0e6c5a2-1111-2222-3333-444455556666"), "a thread id has dashes of its own");
        assert_eq!(thread_in_tag("nothing"), None);
    }

    #[test]
    fn a_thread_waiting_on_its_schedule_hibernates_until_it() {
        use crate::highlights::Schedule;
        let schedule = |id: &str, thread: &str, next: &str, paused: bool| Schedule { id: id.into(), thread_id: Some(thread.into()), next_run_at: Some(next.into()), when_text: "Weekdays at 9:00".into(), paused, ..Default::default() };
        let quiet = thread(1, json!({}));
        let later = thread(2, json!({}));
        let asks = thread(3, json!({ "needs": "you", "needsYou": true, "asks": [{ "action": "Ship it?" }] }));
        let paused = thread(4, json!({}));
        let schedules = [
            schedule("s1", "t1", "2026-11-07T09:00:00Z", false),
            schedule("s1b", "t1", "2026-11-06T13:00:00Z", false),
            schedule("s2", "t2", "2026-11-06T12:30:00Z", false),
            schedule("s3", "t3", "2026-11-06T13:00:00Z", false),
            schedule("s4", "t4", "2026-11-06T13:00:00Z", true),
        ];
        let wakes = wakes(&schedules);
        assert_eq!(wakes["t1"].at_ms, parse_ms("2026-11-06T13:00:00Z").unwrap(), "the soonest of its schedules");
        assert!(!wakes.contains_key("t4"), "a paused schedule wakes nothing");
        let s = sections_ordered(&[quiet, later, asks, paused], now(), InboxOrder::NeedsYou, &wakes);
        assert_eq!(ids(&s, "hibernating"), ["t2", "t1"], "soonest to wake first");
        assert_eq!(ids(&s, "inbox"), ["t3", "t4"], "one that needs Mike stays where he'll see it");
    }

    #[test]
    fn threads_go_under_the_headings_the_mac_app_uses() {
        let asks = thread(1, json!({ "needs": "you", "needsYou": true, "asks": [{ "action": "Ship it?" }] }));
        let running = thread(2, json!({ "status": "running" }));
        let done = thread(3, json!({}));
        let asleep = thread(4, json!({ "snoozedUntil": "2026-11-07T00:00:00Z" }));
        let starred = thread(5, json!({ "starredAt": "2026-11-06T09:00:00Z" }));
        let all = [asks, running, done, asleep, starred];
        let s = sections(&all, now());
        assert_eq!(ids(&s, "inbox"), ["t1", "t3"]);
        assert_eq!(ids(&s, "running"), ["t2"]);
        assert_eq!(ids(&s, "hibernating"), ["t4"]);
        assert_eq!(ids(&s, "starred"), ["t5"]);
    }

    #[test]
    fn a_turn_waiting_on_a_permission_card_is_still_busy() {
        // As Swift's ThreadStatus.isBusy: it can be stopped, and isn't idle.
        assert!(thread(1, json!({ "status": "waiting" })).busy());
        assert!(thread(1, json!({ "status": "running" })).busy());
        assert!(!thread(1, json!({ "status": "idle" })).busy());
        assert!(!thread(1, json!({ "status": "error" })).busy());
    }

    #[test]
    fn a_thread_stuck_on_an_answer_comes_first_then_newest() {
        let all = [thread(1, json!({})), thread(2, json!({})), thread(3, json!({ "status": "waiting" }))];
        assert_eq!(ids(&sections(&all, now()), "inbox"), ["t3", "t2", "t1"]);
    }

    #[test]
    fn sub_agents_still_going_keep_it_running() {
        let all = [thread(1, json!({ "needs": "you", "background": [{ "kind": "agent" }] }))];
        assert_eq!(ids(&sections(&all, now()), "running"), ["t1"]);
    }

    #[test]
    fn unread_and_an_empty_inbox() {
        assert!(thread(1, json!({ "lastSeq": 12 })).unread());
        assert!(!thread(1, json!({})).unread());
        let none: [ThreadSummary; 0] = [];
        assert_eq!(sections(&none, now()).iter().map(|s| s.key).collect::<Vec<_>>(), ["inbox"]);
    }

    #[test]
    fn dates_and_folders() {
        assert_eq!(parse_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(parse_ms("2026-11-06T12:00:00Z").unwrap() - parse_ms("2026-11-06T11:00:00Z").unwrap(), 3_600_000);
        assert_eq!(folder_name("/home/mike/dev/x/"), "x");
        assert_eq!(folder_name(r"C:\Users\mike\dev\y"), "y");
        assert_eq!(ago(now() - 5 * 60_000, now()), "5m");
    }

    #[test]
    fn model_names() {
        assert_eq!(model_label("claude", Some("claude-opus-5-5")), "Claude Opus 5.5");
        assert_eq!(model_label("claude", Some("claude-fable-5-1[1m]")), "Claude Fable 5.1");
        assert_eq!(model_label("claude", Some("opus")), "Claude Opus");
        assert_eq!(model_label("codex", Some("gpt-6-astra")), "GPT-6 Astra");
        assert_eq!(model_label("codex", Some("gpt-6.1-sol")), "GPT-6.1 Sol");
        assert_eq!(model_label("claude", None), "Claude");
        assert_eq!(model_label("codex", Some("something-else")), "something-else");
    }
}
