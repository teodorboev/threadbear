// The design review window's rules, with no window in sight: a port of the
// Swift app's ReviewSession (app/Sources/Threadbear/ReviewSession.swift), the
// parts of DesignReview.swift the window uses (ReviewDraft.canSubmit, an
// option's label and shots, ReviewGalleryLayout), the share of the row under
// the stage (ReviewStepperLayout.share), where the window opens
// (ReviewWindowController.frame) and the zoom of the shot on the stage
// (ReviewImageScroll and ReviewMinimap in ReviewWindow.swift).
//
// The session makes no calls of its own. Each save, send and cancel is a
// `begin_…` that says what to send and a `…_done` or `…_failed` that takes
// the answer, so the GPUI window does the waiting and these tests can put an
// edit, or another window's change, between the two, as the Swift tests do
// with continuations.
use crate::reducer::{DesignReview, ImageRef, ReviewDraft, ReviewOption, ReviewShot, Row, RowContent};
use crate::zoom::Rect;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most a note on one option and the reply may be, in UTF-16 units, as
/// the runner counts them (docs/protocol.md, "Design reviews").
pub const NOTE_LIMIT: usize = 4000;
pub const REPLY_LIMIT: usize = 8000;

/// What the window says when the review moved on without it.
pub const CHANGED_ELSEWHERE: &str = "This review changed elsewhere. Your unsent notes are kept here.";
pub const CLOSED_ELSEWHERE: &str = "This review was closed elsewhere. Your unsent notes are still shown below.";

/// The runner restarting answers with these: the connection gone (the
/// client's own errors are -1) or `RPCError.busy`. A call that gets one is
/// tried again for a while, as the runner updates itself several times a day
/// and is gone for a few seconds each time; anything else is a real answer.
pub fn runner_away(code: i64) -> bool {
    code == -1 || code == -32020
}

/// How long a call keeps trying through a restart (`reconnectWait`).
pub const RECONNECT_WAIT_MS: u64 = 20_000;
/// The pause before an edit is saved (`scheduleSave`).
pub const SAVE_AFTER_MS: u64 = 500;
/// How often an open review asks for its latest copy while it's waiting
/// (the window's `.task`): replies from the phone or another window.
pub const REFRESH_EVERY_MS: u64 = 5_000;

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

// ---------------------------------------------------------------- the draft

impl ReviewDraft {
    pub fn has_notes(&self) -> bool {
        self.notes.values().any(|note| !note.trim().is_empty())
    }

    pub fn has_reply(&self) -> bool {
        self.reply.as_deref().is_some_and(|reply| !reply.trim().is_empty())
    }

    /// The runner checks again. This only keeps Send from offering an empty
    /// reply or a pick that can't be made (`canSubmit`).
    pub fn can_submit(&self, options: &[ReviewOption]) -> bool {
        let known = |id: &str| options.iter().any(|o| o.id == id);
        if !self.notes.iter().all(|(id, note)| known(id) && utf16_len(note) <= NOTE_LIMIT) {
            return false;
        }
        if self.reply.as_deref().map_or(0, utf16_len) > REPLY_LIMIT || (self.none && !self.picked_ids.is_empty()) {
            return false;
        }
        if !self.picked_ids.iter().all(|id| options.iter().any(|o| &o.id == id && !o.reference)) {
            return false;
        }
        !self.picked_ids.is_empty() || self.none || self.has_reply() || self.has_notes()
    }

    /// What `reviews.save` and `reviews.respond` take (`jsonValue`): always
    /// the list of picks, so a runner that knows it takes it over `pickedId`,
    /// which is the first pick for one that doesn't.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("notes".into(), Value::Object(self.notes.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        if let Some(first) = self.picked_id() {
            out.insert("pickedId".into(), json!(first));
        }
        out.insert("pickedIds".into(), json!(self.picked_ids));
        if let Some(reply) = &self.reply {
            out.insert("reply".into(), json!(reply));
        }
        if self.none {
            out.insert("none".into(), json!(true));
        }
        Value::Object(out)
    }

    /// As the runner keeps it: notes and the reply trimmed, blank ones gone.
    /// A space left at the end must not keep a draft dirty and set off
    /// another save every half second.
    pub fn normalized(&self) -> ReviewDraft {
        let reply = self.reply.as_deref().unwrap_or("").trim();
        ReviewDraft {
            picked_ids: self.picked_ids.clone(),
            notes: self.notes.iter().filter_map(|(id, note)| (!note.trim().is_empty()).then(|| (id.clone(), note.trim().to_owned()))).collect(),
            reply: (!reply.is_empty()).then(|| reply.to_owned()),
            none: self.none,
        }
    }
}

// ---------------------------------------------------------------- options and shots

impl ReviewOption {
    /// Agents often number their titles already ("1. One decision at a
    /// time"); the number isn't said twice.
    pub fn label(&self) -> String {
        if self.title.starts_with(&format!("{}.", self.id)) || self.title.starts_with(&format!("{} ", self.id)) || self.title == self.id {
            self.title.clone()
        } else {
            format!("{}. {}", self.id, self.title)
        }
    }

    /// What the window steps through: the shots, or the one image.
    pub fn all_shots(&self) -> Vec<ReviewShot> {
        if self.shots.is_empty() {
            vec![ReviewShot { title: None, image: self.image.clone(), media: None }]
        } else {
            self.shots.clone()
        }
    }

    pub fn shot_count(&self) -> usize {
        self.shots.len().max(1)
    }

    /// One of its shots, or its first when there's no such one.
    pub fn shot(&self, index: usize) -> ReviewShot {
        let shots = self.all_shots();
        shots.get(index).cloned().unwrap_or_else(|| shots[0].clone())
    }
}

impl ReviewShot {
    /// The clip to play, when the shot is a video.
    pub fn video(&self) -> Option<&str> {
        self.media.as_ref().filter(|m| m.media_type.starts_with("video/")).map(|m| m.path.as_str())
    }

    /// The page to show, when the shot is one.
    pub fn page(&self) -> Option<&str> {
        self.media.as_ref().filter(|m| m.media_type == "text/html").map(|m| m.path.as_str())
    }
}

/// A shot's tooltip: the option, and which of its shots when it has several.
pub fn shot_tip(option: &ReviewOption, index: usize) -> String {
    let shots = option.all_shots();
    if shots.len() <= 1 {
        return option.label();
    }
    let what = shots.get(index).and_then(|s| s.title.clone()).unwrap_or_else(|| format!("shot {}", index + 1));
    format!("{}, {what}", option.label())
}

/// Under the option's name on the stage: which shot this is, "2 of 3 · Mid-drag".
pub fn caption_where(option: &ReviewOption, shown: &ReviewShot) -> String {
    let shots = option.all_shots();
    let index = shots.iter().position(|s| s == shown).unwrap_or(0);
    let title = shown.title.clone();
    if shots.len() > 1 {
        let mut out = format!("{} of {}", index + 1, shots.len());
        if let Some(title) = title {
            out.push_str(" \u{00B7} ");
            out.push_str(&title);
        }
        out
    } else {
        title.unwrap_or_default()
    }
}

/// An image's shape, or the Swift app's stand-in when the runner didn't say.
pub fn aspect(image: &ImageRef) -> f64 {
    match (image.width, image.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => w as f64 / h as f64,
        _ => 1100. / 760.,
    }
}

// ---------------------------------------------------------------- the review's words

/// Only for a review that's over, as one that's waiting says it on Send.
pub fn status_text(review: &DesignReview) -> &'static str {
    match review.status.as_str() {
        "answered" => "Sent",
        "cancelled" => "Cancelled",
        "superseded" => "Answered in the thread",
        _ => "Closed",
    }
}

/// The header's button for reviews still waiting (`PendingReviewsButton`).
pub fn pending_button_title(count: u64) -> String {
    format!("Review design options ({count})")
}

/// The review a `review:` link names (an id or the start of one), from what
/// follows the scheme (`WorkLinks.reviewId`).
pub fn link_review_id(rest: &str) -> Option<String> {
    let id = rest.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The thread's review with that id, or the only one it's the start of
/// (`WorkLinks.review`): the latest snapshot of each, as rows keep them.
pub fn find_review<'a>(id: &str, rows: impl IntoIterator<Item = &'a Row>) -> Option<&'a DesignReview> {
    let id = id.to_lowercase();
    let reviews: Vec<&DesignReview> = rows
        .into_iter()
        .filter_map(|row| match &row.content {
            RowContent::Review(review) => Some(review),
            _ => None,
        })
        .collect();
    if let Some(exact) = reviews.iter().rev().find(|r| r.id.to_lowercase() == id) {
        return Some(exact);
    }
    let matches: Vec<&&DesignReview> = reviews.iter().filter(|r| r.id.to_lowercase().starts_with(&id)).collect();
    let mut ids: Vec<&str> = matches.iter().map(|r| r.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() == 1 {
        matches.last().map(|r| **r)
    } else {
        None
    }
}

// ---------------------------------------------------------------- the draft kept here

/// What's kept on this computer of a draft not yet saved (`CachedDraft`):
/// what it was typed over, and which window typed it, so a closing window's
/// late save can't wipe a newer window's copy.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedDraft {
    pub revision: i64,
    pub draft: ReviewDraft,
    pub owner: Option<String>,
    pub conflict: bool,
}

/// Every review's unsaved draft, by thread and review (the Swift app's
/// `reviewDraft.<thread>.<review>` defaults). `changed` says the file needs
/// writing again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftCache {
    entries: BTreeMap<String, CachedDraft>,
    changed: bool,
}

pub type SharedDraftCache = Rc<RefCell<DraftCache>>;

impl DraftCache {
    pub fn key(thread_id: &str, review_id: &str) -> String {
        format!("reviewDraft.{thread_id}.{review_id}")
    }

    pub fn shared() -> SharedDraftCache {
        Rc::new(RefCell::new(DraftCache::default()))
    }

    pub fn get(&self, key: &str) -> Option<&CachedDraft> {
        self.entries.get(key)
    }

    pub fn set(&mut self, key: &str, entry: CachedDraft) {
        if self.entries.get(key) != Some(&entry) {
            self.entries.insert(key.to_owned(), entry);
            self.changed = true;
        }
    }

    pub fn remove(&mut self, key: &str) {
        if self.entries.remove(key).is_some() {
            self.changed = true;
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether it's changed since this was last asked.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// What a file of them holds; anything that doesn't read is left out.
    pub fn decode(value: &Value) -> DraftCache {
        let entries = value
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(key, entry)| {
                        let revision = entry["revision"].as_i64()?;
                        let draft = ReviewDraft::decode(&entry["draft"])?;
                        let owner = entry["owner"].as_str().map(str::to_owned);
                        Some((key.clone(), CachedDraft { revision, draft, owner, conflict: entry["conflict"].as_bool().unwrap_or(false) }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        DraftCache { entries, changed: false }
    }

    pub fn encode(&self) -> Value {
        Value::Object(
            self.entries
                .iter()
                .map(|(key, e)| (key.clone(), json!({ "revision": e.revision, "draft": e.draft.to_json(), "owner": e.owner, "conflict": e.conflict })))
                .collect(),
        )
    }
}

// ---------------------------------------------------------------- the session

/// All of them side by side, or one on the stage (`ReviewMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewMode {
    All,
    Single,
}

impl ReviewMode {
    /// As the scripts name it (the Swift raw value).
    pub fn name(self) -> &'static str {
        match self {
            ReviewMode::All => "All",
            ReviewMode::Single => "Single",
        }
    }
}

/// What the title bar's button and the zoom keys ask of the shot on the stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    Fit,
    Actual,
    Closer,
    Further,
}

/// The serial makes asking twice two asks, as the same step can follow itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomRequest {
    pub serial: u64,
    pub step: ZoomStep,
}

/// A save on its way: what was sent, over which revision.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveCall {
    pub revision: i64,
    pub draft: ReviewDraft,
}

/// A reply on its way.
pub type SendCall = SaveCall;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// One open review (`ReviewSession`): the latest copy, the draft as typed,
/// where it's got to with the runner, and what's on the stage.
#[derive(Debug)]
pub struct ReviewSession {
    pub thread_id: String,
    review: DesignReview,
    draft: ReviewDraft,
    dirty: bool,
    saving: bool,
    submitting: bool,
    conflict: bool,
    pub error: Option<String>,
    /// A review that's waiting opens as a walkthrough, one shot at a time
    /// from the first, ending on all of them to choose from (Mike, 2 Nov);
    /// one that's over opens on all of them, as there's nothing to step to.
    pub mode: ReviewMode,
    /// Whether the shot on show is zoomed past fitting its space; the stage
    /// says so, as zooming happens there.
    pub zoomed: bool,
    zoom_request: Option<ZoomRequest>,
    focused: usize,
    shot: usize,
    /// Today's version over the shot on show while Space is held or its
    /// button is on, to flick between them (Mike, 4 Nov). Moving anywhere
    /// turns it off.
    pub peeking: bool,
    cache: SharedDraftCache,
    cache_key: String,
    owner: String,
}

impl ReviewSession {
    pub fn new(thread_id: &str, review: DesignReview, cache: SharedDraftCache) -> ReviewSession {
        let draft = review.response.clone().unwrap_or_else(|| review.draft.clone());
        let mode = if review.is_pending() && !review.options.is_empty() { ReviewMode::Single } else { ReviewMode::All };
        let cache_key = DraftCache::key(thread_id, &review.id);
        let owner = format!("gpui-{}-{}", std::process::id(), NEXT_OWNER.fetch_add(1, Ordering::Relaxed));
        let mut session = ReviewSession {
            thread_id: thread_id.to_owned(),
            review,
            draft,
            dirty: false,
            saving: false,
            submitting: false,
            conflict: false,
            error: None,
            mode,
            zoomed: false,
            zoom_request: None,
            focused: 0,
            shot: 0,
            peeking: false,
            cache,
            cache_key,
            owner,
        };
        // A draft kept here from before: it's what was typed, so it wins, and
        // if the review has moved on since, it's in conflict.
        let cached = session.cache.borrow().get(&session.cache_key).cloned();
        if let Some(cached) = cached.filter(|c| c.draft != session.draft) {
            session.draft = cached.draft;
            session.dirty = true;
            if cached.conflict || cached.revision != session.review.revision || !session.review.is_pending() {
                session.conflict = true;
                session.error = Some(CHANGED_ELSEWHERE.to_owned());
            }
            session.cache_draft(true);
        }
        session
    }

    pub fn review(&self) -> &DesignReview {
        &self.review
    }

    pub fn draft(&self) -> &ReviewDraft {
        &self.draft
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn submitting(&self) -> bool {
        self.submitting
    }

    pub fn conflict(&self) -> bool {
        self.conflict
    }

    pub fn focused(&self) -> usize {
        self.focused
    }

    pub fn shot(&self) -> usize {
        self.shot
    }

    pub fn zoom_request(&self) -> Option<ZoomRequest> {
        self.zoom_request
    }

    pub fn can_submit(&self) -> bool {
        self.review.is_pending() && !self.submitting && !self.conflict && self.draft.can_submit(&self.review.options)
    }

    pub fn editable(&self) -> bool {
        self.review.is_pending() && !self.submitting
    }

    /// What's sent or kept: the reply as sent once it's over.
    pub fn shown_draft(&self) -> &ReviewDraft {
        self.review.response.as_ref().unwrap_or(&self.draft)
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        match self.mode {
            ReviewMode::All => (0..self.review.options.len()).collect(),
            ReviewMode::Single if self.focused < self.review.options.len() => vec![self.focused],
            ReviewMode::Single => vec![],
        }
    }

    /// Says what will go, so a pick doesn't look like the end of it: it read
    /// as done, with Send at the far end of the window (Mike, 1 Nov).
    pub fn send_label(&self) -> String {
        if self.submitting {
            return "Sending…".to_owned();
        }
        if !self.draft.picked_ids.is_empty() {
            return format!("Send {}", self.draft.picked_ids.join(" + "));
        }
        if self.draft.none {
            return "Send none of them".to_owned();
        }
        if self.draft.has_reply() || self.draft.has_notes() {
            "Send reply".to_owned()
        } else {
            "Send".to_owned()
        }
    }

    pub fn focused_option(&self) -> Option<&ReviewOption> {
        self.review.options.get(self.focused)
    }

    pub fn focused_shot(&self) -> Option<ReviewShot> {
        let option = self.focused_option()?;
        Some(option.shot(self.shot))
    }

    /// What there is to flick to: the first reference option, which agents
    /// use for today's version.
    pub fn baseline(&self) -> Option<&ReviewOption> {
        self.review.options.iter().find(|o| o.reference)
    }

    pub fn can_peek(&self) -> bool {
        self.mode == ReviewMode::Single && self.baseline().is_some_and(|b| Some(&b.id) != self.focused_option().map(|o| &o.id))
    }

    /// The option the stage shows: today's while peeking.
    pub fn shown_option(&self) -> Option<&ReviewOption> {
        if self.peeking && self.can_peek() {
            self.baseline()
        } else {
            self.focused_option()
        }
    }

    /// The shot the stage shows. While peeking, today's shot of the same
    /// name, or its first if none has that name.
    pub fn shown_shot(&self) -> Option<ReviewShot> {
        let baseline = self.baseline().filter(|_| self.peeking && self.can_peek());
        let Some(baseline) = baseline else { return self.focused_shot() };
        let shots = baseline.all_shots();
        let title = self.focused_shot().and_then(|s| s.title);
        title.and_then(|t| shots.iter().find(|s| s.title.as_deref() == Some(t.as_str())).cloned()).or_else(|| shots.first().cloned())
    }

    // Editing. Each says whether the draft changed: if it did, a save is due
    // in `SAVE_AFTER_MS`, unless `autosaves` says it's in conflict.

    pub fn set_reply(&mut self, text: &str) -> bool {
        if !self.editable() {
            return false;
        }
        let mut next = self.draft.clone();
        next.reply = (!text.is_empty()).then(|| text.to_owned());
        self.edit(next)
    }

    pub fn set_note(&mut self, text: &str, id: &str) -> bool {
        if !self.editable() {
            return false;
        }
        let mut next = self.draft.clone();
        next.notes.insert(id.to_owned(), text.to_owned());
        self.edit(next)
    }

    /// Adds or takes away one pick. Several can be picked ("A + B's to-do
    /// list", Mike, 2 Nov); they stay in the review's order, as the runner
    /// keeps them, so the draft doesn't look changed after a save.
    pub fn pick(&mut self, id: &str) -> bool {
        if !self.editable() || !self.review.options.iter().any(|o| o.id == id && !o.reference) {
            return false;
        }
        let mut picked: Vec<String> = self.draft.picked_ids.clone();
        if let Some(at) = picked.iter().position(|p| p == id) {
            picked.remove(at);
        } else {
            picked.push(id.to_owned());
        }
        let mut next = self.draft.clone();
        next.picked_ids = self.review.options.iter().map(|o| o.id.clone()).filter(|o| picked.contains(o)).collect();
        next.none = false;
        self.edit(next)
    }

    /// None of them will do: it takes the picks away, and a pick takes it away.
    pub fn toggle_none(&mut self) -> bool {
        if !self.editable() {
            return false;
        }
        let mut next = self.draft.clone();
        next.none = !next.none;
        if next.none {
            next.picked_ids.clear();
        }
        self.edit(next)
    }

    fn edit(&mut self, next: ReviewDraft) -> bool {
        if next == self.draft {
            return false;
        }
        self.draft = next;
        self.dirty = true;
        self.cache_draft(true);
        true
    }

    /// Whether an edit saves by itself (`scheduleSave`): not while it's in
    /// conflict, when only Save my draft does.
    pub fn autosaves(&self) -> bool {
        !self.conflict
    }

    // The copy kept here.

    fn cache_draft(&mut self, force: bool) {
        let mut cache = self.cache.borrow_mut();
        // A closing window may finish its save after a new window has edited
        // this review; its old session leaves the new copy alone.
        if !force && cache.get(&self.cache_key).is_some_and(|c| c.owner.as_deref() != Some(self.owner.as_str())) {
            return;
        }
        let entry = CachedDraft { revision: self.review.revision, draft: self.draft.clone(), owner: Some(self.owner.clone()), conflict: self.conflict };
        cache.set(&self.cache_key, entry);
    }

    fn clear_cache(&mut self) {
        let mut cache = self.cache.borrow_mut();
        if cache.get(&self.cache_key).and_then(|c| c.owner.as_deref()) == Some(self.owner.as_str()) {
            cache.remove(&self.cache_key);
        }
    }

    // Saving.

    /// A save to send, if one's due (`saveNow`'s guard).
    pub fn begin_save(&mut self) -> Option<SaveCall> {
        if !self.dirty || !self.review.is_pending() || self.saving || self.submitting || self.conflict {
            return None;
        }
        self.saving = true;
        Some(SaveCall { revision: self.review.revision, draft: self.draft.clone() })
    }

    /// The runner saved it. An edit made meanwhile keeps the draft dirty, and
    /// `save_again` says to send it too.
    pub fn save_done(&mut self, saved: DesignReview) {
        let newer = saved.revision >= self.review.revision;
        let stale = saved.revision < self.review.revision;
        if newer {
            self.review = saved;
        }
        self.dirty = self.draft.normalized() != *self.review.response.as_ref().unwrap_or(&self.review.draft);
        if stale && self.dirty {
            self.conflict = true;
            self.error = Some(CHANGED_ELSEWHERE.to_owned());
        } else {
            self.error = None;
        }
        if self.dirty {
            self.cache_draft(false);
        } else {
            self.clear_cache();
        }
        self.saving = false;
    }

    /// The save failed; the window then asks for the latest copy and hands it
    /// to `save_failed_then_loaded`. The draft stays here either way.
    pub fn save_failed(&mut self, message: &str) {
        self.error = Some(format!("Couldn’t save the draft. Your notes are kept on this computer. {message}"));
        self.saving = false;
    }

    pub fn save_failed_then_loaded(&mut self, call: &SaveCall, latest: DesignReview) {
        let revision = latest.revision;
        let theirs = latest.draft.clone();
        self.receive(latest);
        if revision > call.revision && theirs != call.draft.normalized() && theirs != self.draft.normalized() {
            self.conflict = true;
            self.error = Some(CHANGED_ELSEWHERE.to_owned());
            self.cache_draft(false);
        }
    }

    /// After a save lands: a newer keystroke may have come while it was on
    /// its way.
    pub fn save_again(&self) -> bool {
        self.dirty && self.error.is_none() && self.autosaves()
    }

    /// Save my draft: what's here, over the copy that changed elsewhere.
    pub fn keep_my_draft(&mut self) -> bool {
        if !self.review.is_pending() {
            return false;
        }
        self.conflict = false;
        self.error = None;
        self.dirty = true;
        true
    }

    // The latest copy.

    /// A newer copy from the runner: the card's event, a poll, an answer.
    /// What's typed and not saved stays, in conflict if the review moved.
    pub fn receive(&mut self, latest: DesignReview) {
        if latest.id != self.review.id || latest.revision < self.review.revision {
            return;
        }
        let advanced = latest.revision > self.review.revision;
        self.review = latest;
        if !self.dirty {
            self.draft = self.review.response.clone().unwrap_or_else(|| self.review.draft.clone());
        } else if advanced && !self.saving && !self.submitting {
            self.conflict = true;
            self.error = Some(if self.review.is_pending() { CHANGED_ELSEWHERE } else { CLOSED_ELSEWHERE }.to_owned());
            self.cache_draft(false);
        }
    }

    /// A poll that failed: a runner that's restarting is asked again in a
    /// few seconds, with nothing said.
    pub fn refresh_failed(&mut self, code: i64, message: &str) {
        if !runner_away(code) {
            self.error = Some(format!("Couldn’t load the review. {message}"));
        }
    }

    // Sending.

    /// Send was pressed: nothing more can be typed while it goes. The window
    /// waits for a save on its way (it owns the revision), then asks
    /// `send_call` for what to send.
    pub fn begin_submit(&mut self) -> bool {
        if !self.can_submit() {
            return false;
        }
        self.submitting = true;
        true
    }

    pub fn send_call(&mut self) -> Option<SendCall> {
        if !self.review.is_pending() || self.conflict {
            self.submitting = false;
            return None;
        }
        Some(SaveCall { revision: self.review.revision, draft: self.draft.clone() })
    }

    pub fn submit_done(&mut self, answered: DesignReview) {
        self.draft = answered.response.clone().unwrap_or_else(|| answered.draft.clone());
        self.review = answered;
        self.dirty = false;
        self.conflict = false;
        self.error = None;
        self.clear_cache();
        self.submitting = false;
    }

    /// Sending failed. The window asks for the latest copy, and hands it (or
    /// None if that failed too) to `submit_failed_then_loaded`.
    pub fn submit_failed(&mut self, message: &str) {
        self.error = Some(format!("Couldn’t send the reply. Your notes are kept here. {message}"));
    }

    pub fn submit_failed_then_loaded(&mut self, call: &SendCall, latest: Option<DesignReview>) {
        if let Some(latest) = latest {
            // The answer can be lost after the runner kept the reply: its
            // answered copy is the authority.
            if latest.status == "answered" && latest.response.as_ref() == Some(&self.draft.normalized()) {
                self.review = latest;
                self.dirty = false;
                self.clear_cache();
                self.error = None;
            } else {
                self.submitting = false;
                let (pending, revision, theirs) = (latest.is_pending(), latest.revision, latest.draft.clone());
                self.receive(latest);
                if pending && revision > call.revision && self.draft.normalized() != theirs {
                    self.conflict = true;
                    self.error = Some(CHANGED_ELSEWHERE.to_owned());
                    self.cache_draft(false);
                }
            }
        }
        self.submitting = false;
    }

    // Cancelling.

    pub fn begin_cancel(&mut self) -> bool {
        if !self.editable() {
            return false;
        }
        self.submitting = true;
        true
    }

    pub fn cancel_revision(&self) -> i64 {
        self.review.revision
    }

    pub fn cancel_done(&mut self, cancelled: DesignReview) {
        self.review = cancelled;
        self.dirty = false;
        self.clear_cache();
        self.error = None;
        self.submitting = false;
    }

    pub fn cancel_failed(&mut self, message: &str) {
        self.error = Some(format!("Couldn’t cancel the review. {message}"));
        self.submitting = false;
    }

    // What's on the stage.

    pub fn enlarge(&mut self, index: usize, shot: usize) {
        let Some(option) = self.review.options.get(index) else { return };
        self.shot = shot.min(option.shot_count() - 1);
        self.focused = index;
        self.mode = ReviewMode::Single;
        self.peeking = false;
    }

    pub fn zoom(&mut self, step: ZoomStep) -> ZoomRequest {
        let request = ZoomRequest { serial: self.zoom_request.map_or(1, |r| r.serial + 1), step };
        self.zoom_request = Some(request);
        request
    }

    /// Zoom only means anything on an enlarged shot, so it doesn't outlive one.
    pub fn show_all(&mut self) {
        self.mode = ReviewMode::All;
        self.zoomed = false;
        self.peeking = false;
    }

    /// The left and right arrows and the stage's back and next: every shot
    /// of every option in order, then all of them to choose from. It stops at
    /// either end rather than wrapping, so the walkthrough has a start and an
    /// end. From all of them, back goes to the last shot and 0 (Return) to
    /// the one last looked at.
    pub fn browse(&mut self, direction: i32) {
        let count = self.review.options.len();
        if count == 0 {
            return;
        }
        if self.mode == ReviewMode::All {
            if direction < 0 {
                let last = self.review.options[count - 1].shot_count() - 1;
                self.enlarge(count - 1, last);
            } else if direction == 0 {
                let (focused, shot) = (self.focused.min(count - 1), self.shot);
                self.enlarge(focused, shot);
            }
            return;
        }
        let shots = self.review.options[self.focused].shot_count();
        if direction > 0 {
            if self.shot + 1 < shots {
                self.enlarge(self.focused, self.shot + 1);
            } else if self.focused + 1 < count {
                self.enlarge(self.focused + 1, 0);
            } else {
                self.show_all();
            }
        } else if direction < 0 {
            if self.shot > 0 {
                self.enlarge(self.focused, self.shot - 1);
            } else if self.focused > 0 {
                let last = self.review.options[self.focused - 1].shot_count() - 1;
                self.enlarge(self.focused - 1, last);
            }
        }
    }

    /// The up and down arrows: straight to the option before or after,
    /// past the shots in between; down from the last is all of them.
    pub fn jump(&mut self, direction: i32) {
        let count = self.review.options.len();
        if count == 0 {
            return;
        }
        if self.mode == ReviewMode::All {
            if direction < 0 {
                self.enlarge(count - 1, 0);
            }
            return;
        }
        let next = self.focused as i64 + direction as i64;
        if next >= count as i64 {
            self.show_all();
        } else if next >= 0 {
            self.enlarge(next as usize, 0);
        } else {
            self.enlarge(0, 0);
        }
    }

    pub fn can_step_back(&self) -> bool {
        match self.mode {
            ReviewMode::All => !self.review.options.is_empty(),
            ReviewMode::Single => self.focused > 0 || self.shot > 0,
        }
    }

    pub fn can_step_forward(&self) -> bool {
        self.mode == ReviewMode::Single
    }

    /// The next step is all of them, where the pick happens.
    pub fn next_is_all(&self) -> bool {
        self.mode == ReviewMode::Single
            && self.focused + 1 == self.review.options.len()
            && self.shot + 1 >= self.focused_option().map_or(1, ReviewOption::shot_count)
    }
}

// ---------------------------------------------------------------- how the stage is shared

/// How a review's images share the stage (`ReviewGalleryLayout`): same-sized
/// cells, as many columns as make the images biggest. Three phones want one
/// row and four wide mockups two; a fixed two columns left phones tiny in
/// wide cells with a quarter of the window empty (Mike, 1 Nov).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GalleryLayout {
    pub columns: usize,
    pub rows: usize,
    pub cell_width: f64,
    pub cell_height: f64,
}

impl GalleryLayout {
    /// `aspects` is each image's width over height; `chrome_width` and
    /// `chrome_height` are what a cell spends on anything but its image.
    pub fn best(aspects: &[f64], width: f64, height: f64, spacing: f64, chrome_width: f64, chrome_height: f64) -> GalleryLayout {
        let count = aspects.len().max(1);
        let mut best: Option<GalleryLayout> = None;
        let mut best_area = -1.;
        for columns in 1..=count {
            let rows = count.div_ceil(columns);
            let cell_width = ((width - spacing * (columns - 1) as f64) / columns as f64).max(0.);
            let cell_height = ((height - spacing * (rows - 1) as f64) / rows as f64).max(0.);
            let room = ((cell_width - chrome_width).max(0.), (cell_height - chrome_height).max(0.));
            let area: f64 = aspects
                .iter()
                .map(|&aspect| {
                    let (w, h) = Self::fit(aspect, room.0, room.1);
                    w * h
                })
                .sum();
            // Ties keep the fewer columns, for the wider cells.
            if area > best_area {
                best_area = area;
                best = Some(GalleryLayout { columns, rows, cell_width, cell_height });
            }
        }
        best.unwrap_or(GalleryLayout { columns: 1, rows: 1, cell_width: 0., cell_height: 0. })
    }

    /// The biggest size of that shape in the room given.
    pub fn fit(aspect: f64, width: f64, height: f64) -> (f64, f64) {
        if aspect <= 0. || width <= 0. || height <= 0. {
            return (width.max(0.), height.max(0.));
        }
        if width / aspect <= height {
            (width, width / aspect)
        } else {
            (height * aspect, height)
        }
    }
}

/// The row under the stage (`ReviewStepperLayout.share`): the arrows at its
/// ends, and between them the film strip from the left and the marks from
/// the right. Each gets all it wants if there's room; if not, whichever wants
/// less than half keeps it and the other has the rest. Halving it whatever
/// they wanted scrolled a short row of marks for no reason.
pub fn stepper_share(room: f64, strip: f64, marks: f64) -> (f64, f64) {
    let room = room.max(0.);
    let given = marks.min((room / 2.).max(room - strip));
    (strip.min(room - given), given)
}

/// Where a review's window opens (`ReviewWindowController.frame`): 90% of
/// the screen, centred, so a border of the desktop shows round it; no bigger
/// than `REVIEW_WINDOW_MOST` on a huge display, where 90% would be a wall of
/// image. Beside the thread as a tab the images were too small to compare
/// (Mike, 30 Oct).
pub const REVIEW_WINDOW_MOST: (f64, f64) = (2400., 1500.);
pub const REVIEW_WINDOW_LEAST: (f64, f64) = (780., 650.);

pub fn window_frame(visible: Rect) -> Rect {
    let width = (visible.width * 0.9).min(REVIEW_WINDOW_MOST.0);
    let height = (visible.height * 0.9).min(REVIEW_WINDOW_MOST.1);
    let (mid_x, mid_y) = (visible.x + visible.width / 2., visible.y + visible.height / 2.);
    Rect { x: (mid_x - width / 2.).round(), y: (mid_y - height / 2.).round(), width: width.round(), height: height.round() }
}

// ---------------------------------------------------------------- the shot zoomed in

/// A point or a size in the stage's points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// One phase of a scroll, as the platform says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPhase {
    Started,
    Moved,
    Ended,
}

/// The shot on the stage, zoomed and moved around (`ReviewImageScroll`).
/// The image is `doc` points big at one image pixel to a screen pixel (its
/// pixels over the display's scale), shown at `magnification`, and `origin`
/// is the stage's top left in the image's points, as a clip view's bounds
/// are. Fit is where it starts. A mouse wheel zooms, as does a trackpad
/// swipe that starts while it fits, as there's nothing to scroll then (Mike,
/// 5 Nov: "scroll in and out"); zoomed in, a swipe moves around, as do
/// dragging and the minimap.
#[derive(Clone, Debug, PartialEq)]
pub struct StageZoom {
    doc: Point,
    view: Point,
    pub magnification: f64,
    origin: Point,
    fitted: bool,
    /// The swipe under way zooms (true) or moves around, decided as it
    /// starts, so it doesn't turn from one to the other once it's zoomed.
    gesture: Option<bool>,
    /// Which way the swipe zooms in: 1 or -1, 0 until it's moved.
    gesture_direction: f64,
    /// The swipe's finger lifted: what comes now is its coast.
    coasting: bool,
    drag: Option<(Point, Point)>,
}

/// Where the minimap goes in the stage, and the box for the part on show in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minimap {
    pub frame: Rect,
    pub shown: Rect,
}

impl StageZoom {
    /// The most zoomed in: 4 pixels on screen to one in the image.
    const FURTHEST: f64 = 4.;
    const KEY_STEP: f64 = 1.5;
    const WHEEL_STEP: f64 = 0.2;
    const TRACKPAD_STEP: f64 = 0.012;

    pub fn new(doc_width: f64, doc_height: f64, view_width: f64, view_height: f64) -> StageZoom {
        let mut zoom = StageZoom {
            doc: Point { x: doc_width.max(1.), y: doc_height.max(1.) },
            view: Point { x: view_width.max(0.), y: view_height.max(0.) },
            magnification: 1.,
            origin: Point::default(),
            fitted: true,
            gesture: None,
            gesture_direction: 0.,
            coasting: false,
            drag: None,
        };
        zoom.magnification = zoom.fit_scale();
        zoom.constrain();
        zoom
    }

    /// The scale that shows the whole image; a hair under, as rounding the
    /// other way would show scroll bars on an image with nothing to scroll.
    pub fn fit_scale(&self) -> f64 {
        if self.view.x <= 0. || self.view.y <= 0. {
            return 1.;
        }
        (self.view.x / self.doc.x).min(self.view.y / self.doc.y) * 0.9999
    }

    fn least(&self) -> f64 {
        self.fit_scale()
    }

    /// A small image on a big stage needs more than 4× to fit, so the most
    /// is at least twice that.
    fn most(&self) -> f64 {
        (self.fit_scale() * 2.).max(Self::FURTHEST)
    }

    fn clamp(&self, magnification: f64) -> f64 {
        magnification.max(self.least()).min(self.most())
    }

    pub fn is_fitted(&self) -> bool {
        self.fitted
    }

    pub fn is_zoomed(&self) -> bool {
        self.magnification > self.fit_scale() * 1.001
    }

    pub fn doc_size(&self) -> Point {
        self.doc
    }

    pub fn view_size(&self) -> Point {
        self.view
    }

    /// The part of the image on show, in its points (`contentView.bounds`).
    pub fn visible(&self) -> Rect {
        let m = self.magnification.max(1e-9);
        Rect { x: self.origin.x, y: self.origin.y, width: self.view.x / m, height: self.view.y / m }
    }

    /// Where the image is drawn on the stage, in the stage's points.
    pub fn image_rect(&self) -> Rect {
        let m = self.magnification;
        Rect { x: -self.origin.x * m, y: -self.origin.y * m, width: self.doc.x * m, height: self.doc.y * m }
    }

    /// A smaller image is held in the middle of the stage, and a bigger one
    /// can't be moved past its edges (`ReviewCentringClip`).
    fn constrain(&mut self) {
        let visible = self.visible();
        self.origin.x = if visible.width > self.doc.x { (self.doc.x - visible.width) / 2. } else { self.origin.x.max(0.).min(self.doc.x - visible.width) };
        self.origin.y = if visible.height > self.doc.y { (self.doc.y - visible.height) / 2. } else { self.origin.y.max(0.).min(self.doc.y - visible.height) };
    }

    fn settle(&mut self) {
        self.fitted = self.magnification <= self.fit_scale() * 1.001;
        self.constrain();
    }

    /// The stage changed size: fitted stays fitted, zoomed keeps its scale
    /// and its top left.
    pub fn set_view(&mut self, width: f64, height: f64) {
        let view = Point { x: width.max(0.), y: height.max(0.) };
        if self.view == view {
            return;
        }
        self.view = view;
        self.magnification = if self.fitted { self.fit_scale() } else { self.clamp(self.magnification) };
        self.settle();
    }

    /// A different image of the same place (today's version, flicked to):
    /// the same part of it stays on show at the same zoom, or there'd be
    /// nothing to compare. Its size can differ.
    pub fn swap_image(&mut self, doc_width: f64, doc_height: f64) {
        let centre = self.centre_fraction();
        let fitted = self.fitted;
        self.doc = Point { x: doc_width.max(1.), y: doc_height.max(1.) };
        self.magnification = if fitted { self.fit_scale() } else { self.clamp(self.magnification) };
        self.fitted = fitted;
        if !fitted {
            self.go_to(centre.x, centre.y);
        }
        self.settle();
    }

    fn centre_fraction(&self) -> Point {
        let v = self.visible();
        Point { x: (v.x + v.width / 2.) / self.doc.x.max(1.), y: (v.y + v.height / 2.) / self.doc.y.max(1.) }
    }

    pub fn apply(&mut self, step: ZoomStep) {
        match step {
            ZoomStep::Fit => self.magnification = self.fit_scale(),
            ZoomStep::Actual => {
                // One image pixel to one on screen, from the top of the page.
                // An image smaller than the stage is already past that, so it
                // doubles.
                let fit = self.fit_scale();
                self.magnification = self.clamp(if fit < 1. { 1. } else { fit * 2. });
                let visible = self.visible();
                self.origin = Point { x: ((self.doc.x - visible.width) / 2.).max(0.), y: 0. };
            }
            ZoomStep::Closer => self.zoom_by(Self::KEY_STEP, None),
            ZoomStep::Further => self.zoom_by(1. / Self::KEY_STEP, None),
        }
        self.settle();
    }

    /// Zooms keeping the spot under the pointer under it (`at`, in the
    /// stage's points), or the middle of the stage without one.
    pub fn zoom_by(&mut self, factor: f64, at: Option<Point>) {
        let before = self.visible();
        let at = at.unwrap_or(Point { x: self.view.x / 2., y: self.view.y / 2. });
        let across = (at.x / self.view.x.max(1.)).clamp(0., 1.);
        let down = (at.y / self.view.y.max(1.)).clamp(0., 1.);
        let anchor = Point { x: before.x + across * before.width, y: before.y + down * before.height };
        self.magnification = self.clamp(self.magnification * factor);
        let after = self.visible();
        self.origin = Point { x: anchor.x - across * after.width, y: anchor.y - down * after.height };
        self.settle();
    }

    /// Puts the stage's middle on a spot, as fractions of the image.
    pub fn go_to(&mut self, x: f64, y: f64) {
        let visible = self.visible();
        self.origin = Point { x: x * self.doc.x - visible.width / 2., y: y * self.doc.y - visible.height / 2. };
        self.constrain();
    }

    /// Moves around by a scroll's worth, in the stage's points: the image
    /// goes the way the fingers do, as in a scroll view.
    pub fn scroll_by(&mut self, dx: f64, dy: f64) {
        let m = self.magnification.max(1e-9);
        self.origin = Point { x: self.origin.x - dx / m, y: self.origin.y - dy / m };
        self.constrain();
    }

    /// A mouse wheel's notches: it zooms, as a wheel has no other way to and
    /// dragging moves around. With Shift it scrolls sideways (that's how a
    /// wheel does), and says it didn't zoom.
    pub fn wheel(&mut self, lines_y: f64, shift: bool, at: Option<Point>) -> bool {
        if shift {
            return false;
        }
        self.zoom_by((lines_y * Self::WHEEL_STEP).exp(), at);
        true
    }

    /// A trackpad's swipe, in points (`scrollingDeltaY`, positive down the
    /// page). Decided once a swipe: one that starts while the image fits
    /// zooms to its end, and its coast does nothing; otherwise the same swipe
    /// would turn from zooming into scrolling as soon as it zoomed. With a
    /// key held (⌘, ⌃ or ⌥) a swipe always zooms, in the way it goes.
    pub fn swipe(&mut self, dx: f64, dy: f64, phase: ScrollPhase, held: bool, at: Option<Point>) {
        let (zooms, mut direction) = match phase {
            ScrollPhase::Started => {
                self.gesture = Some(held || self.fitted);
                self.gesture_direction = if held { 1. } else { 0. };
                self.coasting = false;
                (held || self.fitted, self.gesture_direction)
            }
            ScrollPhase::Ended => {
                self.coasting = true;
                return;
            }
            // The coast after the fingers lift (the platform gives it no
            // phase of its own): nothing after a zoom, the rest of the move
            // after a move.
            ScrollPhase::Moved if self.coasting => {
                if self.gesture != Some(true) {
                    self.scroll_by(dx, dy);
                }
                return;
            }
            ScrollPhase::Moved => match self.gesture {
                Some(zooms) => (zooms, self.gesture_direction),
                // A smooth scroll with no swipe round it: decided each time.
                None => (held || self.fitted, if held { 1. } else { 0. }),
            },
        };
        if !zooms {
            self.scroll_by(dx, dy);
            return;
        }
        // From fit either way goes in, as out is nowhere: with natural
        // scrolling the swipe up you'd use to read on would do nothing.
        if direction == 0. && dy != 0. {
            direction = if dy > 0. { 1. } else { -1. };
            if self.gesture.is_some() {
                self.gesture_direction = direction;
            }
        }
        self.zoom_by((dy * direction * Self::TRACKPAD_STEP).exp(), at);
    }

    /// A pinch, as the platform's magnification (0.1 is 10% closer).
    pub fn pinch(&mut self, delta: f64, at: Option<Point>) {
        self.zoom_by((1. + delta).max(0.05), at);
    }

    /// A press on the stage. Fitted, it's a click (the window goes back to
    /// all of them); zoomed, it's for dragging, and a double click fits it.
    /// Says whether it was a click.
    pub fn press(&mut self, at: Point, clicks: usize) -> bool {
        if self.fitted {
            return true;
        }
        if clicks == 2 {
            self.apply(ZoomStep::Fit);
            return false;
        }
        self.drag = Some((at, self.origin));
        false
    }

    /// The image follows the hand.
    pub fn drag_to(&mut self, at: Point) {
        let Some((from, origin)) = self.drag else { return };
        let m = self.magnification.max(1e-9);
        self.origin = Point { x: origin.x - (at.x - from.x) / m, y: origin.y - (at.y - from.y) / m };
        self.constrain();
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The whole image small, bottom right, with a box for the part on show,
    /// while some of it is off the stage (`ReviewMinimap`): on a page 20,000
    /// pixels long a scroll bar says how far down you are, not what's there.
    /// Bottom right, as the Swift window draws it: its frame is worked out in
    /// the scroll view's unflipped points, whatever its comment says.
    pub fn minimap(&self) -> Option<Minimap> {
        let visible = self.visible();
        let overflows = visible.width < self.doc.x - 1. || visible.height < self.doc.y - 1.;
        if !overflows {
            return None;
        }
        let room = (150., (self.view.y - 32.).max(40.));
        let scale = (room.0 / self.doc.x).min(room.1 / self.doc.y);
        let size = ((self.doc.x * scale).round().max(20.), (self.doc.y * scale).round().max(20.));
        // Clear of where scroll bars would run, down the right edge and along
        // the bottom.
        let frame = Rect { x: self.view.x - size.0 - 22., y: self.view.y - size.1 - 16., width: size.0, height: size.1 };
        let (x0, y0) = (visible.x.max(0.), visible.y.max(0.));
        let (x1, y1) = ((visible.x + visible.width).min(self.doc.x), (visible.y + visible.height).min(self.doc.y));
        let fraction = Rect { x: x0 / self.doc.x, y: y0 / self.doc.y, width: (x1 - x0).max(0.) / self.doc.x, height: (y1 - y0).max(0.) / self.doc.y };
        // Tall enough to see and grab even when it's a sliver of the page.
        let height = (fraction.height * size.1).max(6.);
        let shown = Rect { x: fraction.x * size.0, y: (fraction.y * size.1).min(size.1 - height), width: fraction.width * size.0, height };
        Some(Minimap { frame, shown })
    }

    /// A press or a drag on the minimap, at a point in it: goes there.
    pub fn minimap_to(&mut self, at: Point) {
        let Some(map) = self.minimap() else { return };
        let x = (at.x / map.frame.width.max(1.)).clamp(0., 1.);
        let y = (at.y / map.frame.height.max(1.)).clamp(0., 1.);
        self.go_to(x, y);
    }

    /// The visible box as fractions of the image, for the scripts.
    pub fn shown_fraction(&self) -> Rect {
        let v = self.visible();
        Rect { x: v.x / self.doc.x, y: v.y / self.doc.y, width: v.width / self.doc.x, height: v.height / self.doc.y }
    }
}

// ---------------------------------------------------------------- an agent's notes

/// One run of an agent's note as SwiftUI draws it (`AttributedString`'s
/// inline-only markdown, which the Swift rail uses): bold, italic, code and
/// links, the rest as written, blank lines kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// Shared by every run of the link: a link's words in many styles would
    /// otherwise copy its address once a run (Codex's second look at
    /// Daybreak 93).
    pub link: Option<std::sync::Arc<str>>,
}

#[derive(Clone, Copy, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
}

/// The runs a note draws as, next to each other with the same look merged.
///
/// Each delimiter's search for its closer is bounded: a backtick run looks
/// only at the starts of runs, and a search that found nothing isn't made
/// again for the same delimiter further on (anything after it is in what was
/// searched); links' brackets and parentheses are matched once up front; and
/// emphasis and links nest at most `INLINE_DEPTH` deep. Notes are drawn on
/// every render of the review window, and unmatched backticks made the first
/// version take seconds over one long note (Daybreak 93, 11 Nov 2026).
pub fn inline_markdown(source: &str) -> Vec<InlineRun> {
    let chars: Vec<char> = source.chars().collect();
    let marks = Marks::new(&chars);
    let mut out: Vec<InlineRun> = Vec::new();
    inline_runs(&chars, &marks, 0, chars.len(), InlineStyle::default(), None, 0, &mut out);
    let mut merged: Vec<InlineRun> = Vec::new();
    for run in out.into_iter().filter(|r| !r.text.is_empty()) {
        match merged.last_mut() {
            Some(last) if (last.bold, last.italic, last.code, &last.link) == (run.bold, run.italic, run.code, &run.link) => last.text.push_str(&run.text),
            _ => merged.push(run),
        }
    }
    merged
}

/// How deep emphasis and links nest before the rest is taken as words.
const INLINE_DEPTH: usize = 8;

/// A note's brackets and parentheses, found once (`inline_link`).
struct Marks {
    /// For each `[`, the `]` that closes it, as a scan counting depth finds.
    bracket_close: Vec<Option<usize>>,
    /// For each place, the first `)` there or after it (the note's length
    /// for none).
    next_paren: Vec<usize>,
    /// How many whitespace characters come before each place, and the first
    /// one that isn't whitespace at or after each place and the last before
    /// it: whether a link's address has a space is worked out from these,
    /// not by a scan.
    spaces_before: Vec<usize>,
    next_word: Vec<usize>,
    last_word_before: Vec<Option<usize>>,
}

impl Marks {
    fn new(c: &[char]) -> Marks {
        let mut bracket_close = vec![None; c.len()];
        let mut open: Vec<usize> = Vec::new();
        for (j, ch) in c.iter().enumerate() {
            match ch {
                '[' => open.push(j),
                ']' => {
                    if let Some(at) = open.pop() {
                        bracket_close[at] = Some(j);
                    }
                }
                _ => {}
            }
        }
        let mut next_paren = vec![c.len(); c.len() + 1];
        for j in (0..c.len()).rev() {
            next_paren[j] = if c[j] == ')' { j } else { next_paren[j + 1] };
        }
        let mut spaces_before = vec![0; c.len() + 1];
        let mut last_word_before = vec![None; c.len() + 1];
        for (j, ch) in c.iter().enumerate() {
            spaces_before[j + 1] = spaces_before[j] + usize::from(ch.is_whitespace());
            last_word_before[j + 1] = if ch.is_whitespace() { last_word_before[j] } else { Some(j) };
        }
        let mut next_word = vec![c.len(); c.len() + 1];
        for j in (0..c.len()).rev() {
            next_word[j] = if c[j].is_whitespace() { next_word[j + 1] } else { j };
        }
        Marks { bracket_close, next_paren, spaces_before, next_word, last_word_before }
    }
}

fn inline_push(out: &mut Vec<InlineRun>, text: &mut String, style: InlineStyle, code: bool, link: Option<&std::sync::Arc<str>>) {
    if !text.is_empty() {
        out.push(InlineRun { text: std::mem::take(text), bold: style.bold, italic: style.italic, code, link: link.cloned() });
    }
}

fn run_length(c: &[char], at: usize, end: usize, ch: char) -> usize {
    c[at..end].iter().take_while(|&&x| x == ch).count()
}

/// The run's length up to `max`: emphasis only ever needs three, and a long
/// run of `*` counted whole at each of its places is quadratic.
fn run_length_upto(c: &[char], at: usize, end: usize, ch: char, max: usize) -> usize {
    c[at..end].iter().take(max).take_while(|&&x| x == ch).count()
}

/// A delimiter run can open emphasis when what follows it isn't a space, and
/// `_` only at the start of a word, so snake_case stays as it is.
fn can_open(c: &[char], at: usize, len: usize, end: usize) -> bool {
    let next = c.get(at + len).filter(|_| at + len < end);
    let before = at.checked_sub(1).map(|i| c[i]);
    next.is_some_and(|n| !n.is_whitespace()) && (c[at] == '*' || before.is_none_or(|b| !b.is_alphanumeric()))
}

fn can_close(c: &[char], at: usize, len: usize, end: usize) -> bool {
    let before = at.checked_sub(1).map(|i| c[i]);
    let after = c.get(at + len).filter(|_| at + len < end);
    before.is_some_and(|b| !b.is_whitespace()) && (c[at] == '*' || after.is_none_or(|a| !a.is_alphanumeric()))
}

/// The start of the next run of exactly `ticks` backticks from `from`, up
/// to `end`, looking only at where runs start.
fn closing_ticks(c: &[char], from: usize, end: usize, ticks: usize) -> Option<usize> {
    let mut j = from;
    while j < end {
        if c[j] != '`' {
            j += 1;
            continue;
        }
        let len = run_length(c, j, end, '`');
        if len == ticks && (j == 0 || c[j - 1] != '`') {
            return Some(j);
        }
        j += len;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn inline_runs(c: &[char], marks: &Marks, mut i: usize, end: usize, style: InlineStyle, link: Option<&std::sync::Arc<str>>, depth: usize, out: &mut Vec<InlineRun>) {
    let mut plain = String::new();
    // Delimiters whose closer wasn't found from somewhere in this range:
    // none is found from further on either.
    let mut unclosed_ticks: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut unclosed_marks: std::collections::HashSet<(char, usize)> = std::collections::HashSet::new();
    let nests = depth < INLINE_DEPTH;
    while i < end {
        let ch = c[i];
        // A backslash keeps the punctuation after it as it is.
        if ch == '\\' && i + 1 < end && c[i + 1].is_ascii_punctuation() {
            plain.push(c[i + 1]);
            i += 2;
            continue;
        }
        if ch == '`' {
            let ticks = run_length(c, i, end, '`');
            let close = if unclosed_ticks.contains(&ticks) { None } else { closing_ticks(c, i + ticks, end, ticks) };
            if let Some(close) = close {
                inline_push(out, &mut plain, style, false, link);
                let mut code: String = c[i + ticks..close].iter().collect();
                // One space either side is how a code span holds a backtick.
                if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') {
                    code = code[1..code.len() - 1].to_owned();
                }
                inline_push(out, &mut code, style, true, link);
                i = close + ticks;
                continue;
            }
            unclosed_ticks.insert(ticks);
            plain.extend(&c[i..i + ticks]);
            i += ticks;
            continue;
        }
        if ch == '[' && link.is_none() && nests {
            if let Some((text_end, url, after)) = inline_link(c, marks, i, end) {
                inline_push(out, &mut plain, style, false, link);
                let url: std::sync::Arc<str> = url.into();
                inline_runs(c, marks, i + 1, text_end, style, Some(&url), depth + 1, out);
                i = after;
                continue;
            }
        }
        if (ch == '*' || ch == '_') && nests && can_open(c, i, run_length_upto(c, i, end, ch, 3), end) {
            let len = run_length_upto(c, i, end, ch, 3);
            let close = if unclosed_marks.contains(&(ch, len)) {
                None
            } else {
                (i + len..end).find(|&j| c[j] == ch && (j == 0 || c[j - 1] != ch) && run_length_upto(c, j, end, ch, len) >= len && can_close(c, j, len, end))
            };
            if let Some(close) = close {
                inline_push(out, &mut plain, style, false, link);
                let inner = InlineStyle { bold: style.bold || len >= 2, italic: style.italic || len != 2 };
                inline_runs(c, marks, i + len, close, inner, link, depth + 1, out);
                i = close + len;
                continue;
            }
            unclosed_marks.insert((ch, len));
        }
        plain.push(ch);
        i += 1;
    }
    inline_push(out, &mut plain, style, false, link);
}

/// `[words](address)`: where its words end, the address, and where it ends.
fn inline_link(c: &[char], marks: &Marks, at: usize, end: usize) -> Option<(usize, String, usize)> {
    let close = marks.bracket_close[at].filter(|&j| j < end)?;
    if close + 1 >= end || c[close + 1] != '(' {
        return None;
    }
    let stop = Some(marks.next_paren[close + 2]).filter(|&j| j < end)?;
    // The address is what's between the parentheses, trimmed: none if
    // that's empty or has a space in it. Worked out from the counts before
    // anything's copied, as a rejected candidate's copy would be the rest
    // of the note each time.
    let first = marks.next_word[close + 2];
    if first >= stop {
        return None;
    }
    let last = marks.last_word_before[stop]?;
    if marks.spaces_before[last + 1] != marks.spaces_before[first] {
        return None;
    }
    let url: String = c[close + 2..stop].iter().collect::<String>().trim().to_owned();
    Some((close, url, stop + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reducer::ImageRef;

    fn image(name: &str) -> ImageRef {
        ImageRef::new(&format!("/nonexistent/{name}.png"), "image/png")
    }

    fn option(id: &str) -> ReviewOption {
        ReviewOption { id: id.into(), title: format!("Option {id}"), note: Some(format!("Why {id}")), reference: false, image: image(id), shots: vec![], url: None }
    }

    fn shot(title: Option<&str>, name: &str) -> ReviewShot {
        ReviewShot { title: title.map(str::to_owned), image: image(name), media: None }
    }

    fn design(options: Vec<ReviewOption>) -> DesignReview {
        DesignReview {
            id: "review".into(),
            title: "Account picker".into(),
            note: None,
            recommended: None,
            options,
            status: "pending".into(),
            revision: 1,
            created_at: String::new(),
            updated_at: String::new(),
            draft: ReviewDraft::default(),
            response: None,
            reply_turn_id: None,
        }
    }

    /// ReviewSessionTests' two options.
    fn two() -> DesignReview {
        let mut a = option("A");
        a.title = "Auto".into();
        let mut b = option("B");
        b.title = "One picker".into();
        design(vec![a, b])
    }

    /// ReviewWindowTests' review: A has three shots and runs live, B two
    /// untitled, C one, and Today is a reference with two.
    fn rail() -> DesignReview {
        let mut options: Vec<ReviewOption> = ["A", "B", "C"].iter().map(|id| option(id)).collect();
        options[0].shots = vec![shot(Some("Overview"), "A"), shot(Some("Mid-drag"), "A2"), shot(Some("Detail"), "A3")];
        options[0].url = Some("http://localhost:5190/a".into());
        options[1].shots = vec![shot(None, "B"), shot(None, "B2")];
        let mut today = option("Today");
        today.title = "Today".into();
        today.reference = true;
        today.image = image("today");
        today.shots = vec![shot(Some("Overview"), "today"), shot(Some("Mid-drag"), "today2")];
        options.push(today);
        design(options)
    }

    fn session(review: DesignReview) -> ReviewSession {
        ReviewSession::new("thread", review, DraftCache::shared())
    }

    fn at(s: &ReviewSession) -> (String, usize) {
        (s.focused_option().map(|o| o.id.clone()).unwrap_or_default(), s.shot())
    }

    fn saved(mut review: DesignReview, draft: &ReviewDraft) -> DesignReview {
        review.revision += 1;
        review.draft = draft.normalized();
        review
    }

    // The draft.

    #[test]
    fn a_draft_can_be_sent_with_notes_alone_but_not_a_reference_pick() {
        let mut review = two();
        let mut d = option("D");
        d.reference = true;
        review.options.push(d);
        let options = &review.options;
        let draft = |f: &dyn Fn(&mut ReviewDraft)| {
            let mut d = ReviewDraft::default();
            f(&mut d);
            d
        };
        assert!(!ReviewDraft::default().can_submit(options));
        assert!(!draft(&|d| { d.notes.insert("A".into(), " \n ".into()); }).can_submit(options));
        assert!(draft(&|d| { d.notes.insert("D".into(), "Works with every option".into()); }).can_submit(options));
        assert!(draft(&|d| d.picked_ids = vec!["B".into()]).can_submit(options));
        assert!(!draft(&|d| d.picked_ids = vec!["D".into()]).can_submit(options));
        assert!(!draft(&|d| d.picked_ids = vec!["missing".into()]).can_submit(options));
        assert!(!draft(&|d| { d.notes.insert("missing".into(), "Note".into()); }).can_submit(options));
        assert!(!draft(&|d| { d.notes.insert("A".into(), "x".repeat(4001)); }).can_submit(options));
        assert!(!draft(&|d| {
            d.picked_ids = vec!["A".into()];
            d.none = true;
        })
        .can_submit(options), "picks or none, not both");
        assert!(draft(&|d| d.reply = Some("Hmm".into())).can_submit(options));
        assert!(!draft(&|d| d.reply = Some("  ".into())).can_submit(options));
    }

    #[test]
    fn a_draft_goes_with_its_picks_listed_and_reads_back_the_same() {
        let mut draft = ReviewDraft { picked_ids: vec!["A".into(), "D".into()], reply: Some("Both".into()), ..Default::default() };
        draft.notes.insert("A".into(), "Fainter".into());
        let sent = draft.to_json();
        assert_eq!(sent["pickedId"], "A", "the first pick, for a runner from before several");
        assert_eq!(sent["pickedIds"], json!(["A", "D"]));
        assert_eq!(ReviewDraft::decode(&sent), Some(draft));
        let none = ReviewDraft { none: true, ..Default::default() }.to_json();
        assert_eq!(none, json!({ "notes": {}, "pickedIds": [], "none": true }));
    }

    #[test]
    fn an_option_is_one_shot_unless_it_has_more_and_says_its_number_once() {
        let mut a = option("A");
        assert_eq!(a.all_shots().len(), 1);
        assert_eq!(a.label(), "A. Option A");
        a.title = "A. Already numbered".into();
        assert_eq!(a.label(), "A. Already numbered");
        a.title = "A".into();
        assert_eq!(a.label(), "A");
        let review = rail();
        assert_eq!(review.options[0].all_shots().iter().map(|s| s.title.clone()).collect::<Vec<_>>(), vec![Some("Overview".to_owned()), Some("Mid-drag".to_owned()), Some("Detail".to_owned())]);
        assert_eq!(shot_tip(&review.options[0], 1), "A. Option A, Mid-drag");
        assert_eq!(shot_tip(&review.options[1], 1), "B. Option B, shot 2");
        assert_eq!(shot_tip(&review.options[2], 0), "C. Option C");
        assert_eq!(caption_where(&review.options[0], &review.options[0].shot(1)), "2 of 3 \u{00B7} Mid-drag");
        assert_eq!(caption_where(&review.options[1], &review.options[1].shot(0)), "1 of 2");
        assert_eq!(caption_where(&review.options[2], &review.options[2].shot(0)), "");
    }

    #[test]
    fn a_shot_can_be_a_video_or_a_page_with_its_still() {
        let video = ReviewShot { title: None, image: image("a"), media: Some(ImageRef::new("/a.mp4", "video/mp4")) };
        let page = ReviewShot { title: None, image: image("p"), media: Some(ImageRef::new("/p/index.html", "text/html")) };
        assert_eq!((video.video(), video.page()), (Some("/a.mp4"), None));
        assert_eq!((page.video(), page.page()), (None, Some("/p/index.html")));
        assert_eq!(page.image.path, "/nonexistent/p.png", "the still stays the image, for thumbnails");
    }

    // The session, after ReviewSessionTests.

    #[test]
    fn an_edit_during_a_save_is_kept_for_the_next_save() {
        let mut s = session(two());
        s.set_note("First", "A");
        let call = s.begin_save().expect("a save is due");
        s.set_note("Second", "A");
        assert!(s.begin_save().is_none(), "one save at a time");
        s.save_done(saved(s.review().clone(), &call.draft));
        assert_eq!(s.draft().notes["A"], "Second");
        assert!(s.dirty());
        assert_eq!(s.review().revision, 2);
        assert!(s.save_again(), "the newer edit goes next");
    }

    #[test]
    fn a_failed_save_survives_reopening_without_replacing_a_newer_revision() {
        let cache = DraftCache::shared();
        let mut server = two();
        let mut s = ReviewSession::new("thread", server.clone(), cache.clone());
        s.set_note("Keep this text", "B");
        let call = s.begin_save().unwrap();
        s.save_failed("offline");
        s.save_failed_then_loaded(&call, server.clone());
        assert!(s.dirty());
        assert!(s.error.is_some());
        server.revision = 3;
        let reopened = ReviewSession::new("thread", server, cache);
        assert_eq!(reopened.draft().notes["B"], "Keep this text");
        assert_eq!(reopened.review().revision, 3);
        assert!(reopened.conflict());
    }

    #[test]
    fn a_reply_of_notes_alone_goes_once_and_is_then_read_only() {
        let mut s = session(two());
        assert!(!s.can_submit());
        s.set_note("Try a version with a smaller menu", "A");
        assert!(s.can_submit());
        assert!(s.begin_submit());
        assert!(!s.begin_submit(), "Send can't be pressed twice");
        assert!(!s.set_note("Typed while it goes", "A"), "nothing can be typed while it goes");
        let call = s.send_call().unwrap();
        let mut answered = s.review().clone();
        answered.status = "answered".into();
        answered.revision += 1;
        answered.response = Some(call.draft.normalized());
        answered.draft = call.draft.normalized();
        s.submit_done(answered);
        assert!(!s.set_note("Cannot edit an answer", "A"));
        assert!(!s.begin_submit());
        assert_eq!(s.review().response.as_ref().unwrap().notes["A"], "Try a version with a smaller menu");
        assert!(!s.review().is_pending());
        assert!(!s.dirty());
    }

    #[test]
    fn an_answer_from_elsewhere_keeps_what_was_typed_without_sending_it() {
        let mut server = two();
        let mut s = session(server.clone());
        s.set_note("My draft", "A");
        server.status = "answered".into();
        server.revision = 2;
        server.response = Some(ReviewDraft { picked_ids: vec!["B".into()], ..Default::default() });
        s.receive(server);
        assert_eq!(s.draft().notes["A"], "My draft");
        assert!(!s.can_submit());
        assert_eq!(s.error.as_deref(), Some(CLOSED_ELSEWHERE));
    }

    #[test]
    fn the_runner_trimming_a_note_doesnt_leave_the_draft_dirty() {
        let mut s = session(two());
        s.set_note("A note  ", "A");
        s.set_note("  ", "B");
        let call = s.begin_save().unwrap();
        s.save_done(saved(s.review().clone(), &call.draft));
        assert!(!s.dirty());
        assert!(!s.save_again());
    }

    #[test]
    fn a_newer_copy_from_elsewhere_wins_the_revision_and_what_was_typed_is_in_conflict() {
        let cache = DraftCache::shared();
        let initial = two();
        let mut s = ReviewSession::new("thread", initial.clone(), cache.clone());
        s.set_note("Mine", "A");
        let call = s.begin_save().unwrap();
        let mut remote = initial.clone();
        remote.revision = 3;
        remote.draft.notes.insert("A".into(), "Theirs".into());
        s.receive(remote.clone());
        let mut acknowledgement = initial;
        acknowledgement.revision = 2;
        acknowledgement.draft = call.draft.clone();
        s.save_done(acknowledgement);
        assert_eq!(s.review().revision, 3);
        assert_eq!(s.draft().notes["A"], "Mine");
        assert!(s.dirty());
        assert!(s.conflict());
        assert!(!s.can_submit());
        assert!(!s.save_again(), "only Save my draft saves it now");
        let reopened = ReviewSession::new("thread", remote, cache);
        assert!(reopened.conflict());
    }

    #[test]
    fn a_stale_send_cant_quietly_overwrite_another_windows_draft() {
        let mut remote = two();
        let mut s = session(remote.clone());
        s.set_note("Mine", "A");
        assert!(s.begin_submit());
        let call = s.send_call().unwrap();
        remote.revision += 1;
        remote.draft.notes.insert("B".into(), "Theirs".into());
        s.receive(remote.clone());
        s.submit_failed("This review changed.");
        s.submit_failed_then_loaded(&call, Some(remote));
        assert!(s.conflict());
        assert!(!s.can_submit());
        assert!(!s.submitting());
        assert_eq!(s.draft().notes["A"], "Mine");
    }

    #[test]
    fn a_lost_answer_after_the_runner_kept_the_reply_counts_as_sent() {
        let mut s = session(two());
        s.pick("B");
        s.begin_submit();
        let call = s.send_call().unwrap();
        let mut answered = s.review().clone();
        answered.status = "answered".into();
        answered.revision += 1;
        answered.response = Some(call.draft.normalized());
        s.submit_failed("The connection to the runner closed");
        s.submit_failed_then_loaded(&call, Some(answered));
        assert_eq!(s.review().status, "answered");
        assert!(s.error.is_none());
        assert!(!s.dirty());
    }

    #[test]
    fn a_send_that_never_gets_through_says_so_and_keeps_the_draft() {
        let mut s = session(two());
        s.pick("B");
        s.begin_submit();
        let call = s.send_call().unwrap();
        s.submit_failed("Not connected to the runner");
        s.submit_failed_then_loaded(&call, None);
        assert_eq!(s.review().status, "pending");
        assert!(s.error.is_some());
        assert!(!s.submitting(), "Send can be pressed again");
        assert_eq!(s.draft().picked_ids, vec!["B".to_owned()]);
        assert!(s.can_submit());
    }

    #[test]
    fn a_failed_save_after_another_windows_change_still_needs_sorting_out() {
        let mut remote = two();
        let mut s = session(remote.clone());
        s.set_note("Mine", "A");
        let call = s.begin_save().unwrap();
        remote.revision += 1;
        remote.draft.notes.insert("B".into(), "Theirs".into());
        s.receive(remote.clone());
        s.save_failed("stale revision");
        s.save_failed_then_loaded(&call, remote);
        assert!(s.conflict());
        assert!(!s.can_submit());
        assert_eq!(s.draft().notes["A"], "Mine");
        assert!(s.keep_my_draft(), "Save my draft");
        assert!(!s.conflict());
        assert!(s.begin_save().is_some(), "and it saves over theirs");
    }

    #[test]
    fn a_closing_windows_late_save_cant_wipe_a_reopened_windows_newer_copy() {
        let cache = DraftCache::shared();
        let server = two();
        let mut closing = ReviewSession::new("thread", server.clone(), cache.clone());
        closing.set_note("First window", "A");
        let call = closing.begin_save().unwrap();
        let mut reopened = ReviewSession::new("thread", server.clone(), cache.clone());
        reopened.set_note("Second window", "A");
        closing.save_done(saved(server.clone(), &call.draft));
        let recovered = ReviewSession::new("thread", server, cache);
        assert_eq!(recovered.draft().notes["A"], "Second window");
    }

    #[test]
    fn a_poll_says_nothing_while_the_runner_restarts() {
        let mut s = session(two());
        s.refresh_failed(-1, "Not connected to the runner");
        assert!(s.error.is_none(), "it polls again in a few seconds");
        s.refresh_failed(-32020, "Restarting.");
        assert!(s.error.is_none());
        s.refresh_failed(-32602, "Review not found");
        assert!(s.error.is_some());
        assert!(runner_away(-1) && runner_away(-32020) && !runner_away(-32602));
    }

    #[test]
    fn a_cache_keeps_drafts_between_launches() {
        let cache = DraftCache::shared();
        let mut s = ReviewSession::new("thread", two(), cache.clone());
        s.pick("B");
        s.set_reply("Bigger please");
        assert!(cache.borrow_mut().take_changed());
        let file = cache.borrow().encode();
        let read = DraftCache::decode(&file);
        assert_eq!(read.len(), 1);
        let again = ReviewSession::new("thread", two(), Rc::new(RefCell::new(read)));
        assert_eq!(again.draft().picked_ids, vec!["B".to_owned()]);
        assert_eq!(again.draft().reply.as_deref(), Some("Bigger please"));
        assert!(again.dirty() && !again.conflict(), "the same revision, so it just saves");
        assert!(DraftCache::decode(&json!({ "x": { "revision": "one" } })).is_empty());
    }

    // Picking, after ReviewWindowTests.

    #[test]
    fn several_can_be_picked_and_stay_in_their_order() {
        let mut s = session(rail());
        s.pick("C");
        s.pick("A");
        assert_eq!(s.draft().picked_ids, vec!["A".to_owned(), "C".to_owned()]);
        assert_eq!(s.send_label(), "Send A + C");
        assert_eq!(at(&s), ("A".into(), 0), "picking doesn't move the stage");
        s.pick("C");
        assert_eq!(s.draft().picked_ids, vec!["A".to_owned()]);
        assert!(!s.pick("Today"), "a reference can't be picked");
    }

    #[test]
    fn none_takes_the_picks_away_and_a_pick_takes_none_away() {
        let mut s = session(rail());
        s.pick("B");
        s.toggle_none();
        assert!(s.draft().none);
        assert!(s.draft().picked_ids.is_empty());
        assert_eq!(s.send_label(), "Send none of them");
        assert!(s.can_submit());
        s.pick("B");
        assert!(!s.draft().none);
    }

    #[test]
    fn one_reply_box_and_send_says_what_it_will_send() {
        let mut s = session(rail());
        assert!(!s.can_submit());
        assert_eq!(s.send_label(), "Send");
        s.set_reply("Can you explain what these are for?");
        assert_eq!(s.send_label(), "Send reply");
        assert!(s.can_submit(), "words alone are an answer");
        s.pick("C");
        assert_eq!(s.send_label(), "Send C");
        s.set_reply("");
        assert_eq!(s.draft().reply, None, "an empty box is no reply");
        s.begin_submit();
        assert_eq!(s.send_label(), "Sending…");
    }

    #[test]
    fn a_review_thats_over_opens_on_all_of_them_and_takes_nothing() {
        let mut answered = rail();
        answered.status = "answered".into();
        answered.response = Some(ReviewDraft { picked_ids: vec!["A".into(), "C".into()], reply: Some("Both".into()), ..Default::default() });
        let mut s = session(answered);
        assert_eq!(s.mode, ReviewMode::All, "nothing left to step towards");
        assert!(!s.editable());
        assert!(!s.pick("B"));
        assert!(!s.toggle_none());
        assert_eq!(s.shown_draft().reply.as_deref(), Some("Both"));
        assert_eq!(status_text(s.review()), "Sent");
        for (status, words) in [("cancelled", "Cancelled"), ("superseded", "Answered in the thread"), ("someday", "Closed")] {
            let mut r = rail();
            r.status = status.into();
            assert_eq!(status_text(&r), words);
            assert!(!ReviewSession::new("t", r, DraftCache::shared()).editable(), "{status} is read-only");
        }
    }

    // The walkthrough.

    #[test]
    fn it_opens_on_the_first_and_steps_through_every_shot_to_all_of_them() {
        let mut s = session(rail());
        assert_eq!(s.mode, ReviewMode::Single, "a walkthrough, one at a time");
        assert_eq!(at(&s), ("A".into(), 0));
        assert!(!s.can_step_back(), "nothing before the first");
        s.browse(1);
        assert_eq!(at(&s), ("A".into(), 1), "A's own shots come first");
        s.browse(1);
        s.browse(1);
        assert_eq!(at(&s), ("B".into(), 0));
        s.browse(1);
        s.browse(1);
        s.browse(1);
        assert_eq!(at(&s), ("Today".into(), 0), "a reference is a step too");
        s.browse(1);
        assert!(s.next_is_all());
        s.browse(1);
        assert_eq!(s.mode, ReviewMode::All, "it ends on all of them, to choose");
        assert!(!s.can_step_forward());
        s.browse(-1);
        assert_eq!(at(&s), ("Today".into(), 1));
        s.browse(-1);
        s.browse(-1);
        assert_eq!(at(&s), ("C".into(), 0));
        s.browse(-1);
        assert_eq!(at(&s), ("B".into(), 1), "back into B's last shot");
        s.browse(-1);
        s.browse(-1);
        assert_eq!(at(&s), ("A".into(), 2));
        s.show_all();
        s.browse(0);
        assert_eq!(at(&s), ("A".into(), 2), "Return goes back to the one last looked at");
    }

    #[test]
    fn up_and_down_jump_between_options_past_their_shots() {
        let mut s = session(rail());
        s.browse(1);
        s.jump(1);
        assert_eq!(at(&s), ("B".into(), 0));
        s.jump(-1);
        assert_eq!(at(&s), ("A".into(), 0));
        s.jump(-1);
        assert_eq!(at(&s), ("A".into(), 0));
        s.jump(1);
        s.jump(1);
        s.jump(1);
        s.jump(1);
        assert_eq!(s.mode, ReviewMode::All, "down from the last is all of them");
        s.jump(-1);
        assert_eq!(at(&s), ("Today".into(), 0));
    }

    #[test]
    fn a_review_of_one_image_always_has_one_to_show() {
        let mut only = two();
        only.options.truncate(1);
        let mut s = session(only);
        assert_eq!(s.visible_indices(), vec![0]);
        s.browse(1);
        assert_eq!(s.mode, ReviewMode::All, "next from the only image is the choosing step");
        assert_eq!(s.visible_indices(), vec![0]);
        s.browse(1);
        assert_eq!(s.visible_indices(), vec![0], "and next from there stays put");
    }

    #[test]
    fn todays_version_can_be_flicked_to_and_moving_turns_it_off() {
        let mut s = session(rail());
        s.enlarge(0, 1);
        assert!(s.can_peek());
        s.peeking = true;
        assert_eq!(s.shown_option().map(|o| o.id.as_str()), Some("Today"));
        assert_eq!(s.shown_shot().unwrap().image.path, "/nonexistent/today2.png", "today's shot of the same name");
        assert_eq!(at(&s), ("A".into(), 1), "flicking isn't moving");
        s.enlarge(1, 1);
        assert!(!s.peeking);
        s.peeking = true;
        assert_eq!(s.shown_shot().unwrap().image.path, "/nonexistent/today.png", "no shot of that name: today's first");
        s.browse(1);
        assert!(!s.peeking, "moving turns it off");
        s.enlarge(3, 0);
        assert!(!s.can_peek(), "nothing to flick to from today itself");
        s.show_all();
        assert!(!s.can_peek(), "nor among them all");
    }

    #[test]
    fn a_zoom_request_is_a_new_one_each_time() {
        let mut s = session(rail());
        let first = s.zoom(ZoomStep::Actual);
        let second = s.zoom(ZoomStep::Actual);
        assert_ne!(first.serial, second.serial);
        assert_eq!(s.zoom_request(), Some(second));
    }

    // Links and the header.

    #[test]
    fn a_review_link_finds_its_review_by_id_or_a_unique_start() {
        let mut first = two();
        first.id = "review_account_picker".into();
        let mut second = two();
        second.id = "review_account_menu".into();
        let row = |review: DesignReview| Row::new(&format!("review:{}", review.id), 4, None, RowContent::Review(review));
        let rows = vec![row(first.clone()), row(second.clone())];
        assert_eq!(find_review("review_account_picker", &rows).map(|r| r.id.as_str()), Some("review_account_picker"));
        assert_eq!(find_review("REVIEW_ACCOUNT_M", &rows).map(|r| r.id.as_str()), Some("review_account_menu"));
        assert_eq!(find_review("review_account", &rows), None, "two of them start that way");
        assert_eq!(find_review("nothing", &rows), None);
        assert_eq!(link_review_id(" review_account_picker "), Some("review_account_picker".into()));
        assert_eq!(link_review_id("  "), None);
        assert_eq!(pending_button_title(2), "Review design options (2)");
    }

    // How the stage is shared, after DesignReviewTests and ReviewWindowLayoutTests.

    #[test]
    fn phones_go_in_one_row_and_wide_mockups_in_two() {
        let phones = vec![940. / 1800.; 3];
        let layout = GalleryLayout::best(&phones, 1200., 900., 12., 0., 0.);
        assert_eq!((layout.columns, layout.rows), (3, 1));
        let wide = vec![2200. / 1520.; 4];
        let layout = GalleryLayout::best(&wide, 1200., 900., 12., 0., 0.);
        assert_eq!((layout.columns, layout.rows), (2, 2));
        let one = GalleryLayout::best(&[1.5], 1200., 900., 12., 0., 0.);
        assert_eq!((one.columns, one.cell_width, one.cell_height), (1, 1200., 900.));
        let none = GalleryLayout::best(&[1., 1.], 0., 0., 12., 16., 36.);
        assert_eq!((none.cell_width, none.cell_height), (0., 0.), "a stage with no room yet doesn't go negative");
        assert_eq!(GalleryLayout::fit(0.5, 400., 400.), (200., 400.));
        assert_eq!(GalleryLayout::fit(2., 400., 400.), (400., 200.));
        assert_eq!(aspect(&ImageRef::new("/a.png", "image/png")), 1100. / 760., "no size: the usual mockup's");
    }

    #[test]
    fn the_bottom_row_shares_its_room_by_what_each_wants() {
        assert_eq!(stepper_share(600., 200., 300.), (200., 300.));
        assert_eq!(stepper_share(600., 700., 100.), (500., 100.), "short marks don't scroll to make the strip longer");
        assert_eq!(stepper_share(300., 100., 500.), (100., 200.));
        assert_eq!(stepper_share(300., 900., 900.), (150., 150.));
        assert_eq!(stepper_share(-40., 900., 900.), (0., 0.));
    }

    #[test]
    fn the_window_opens_on_ninety_percent_of_the_screen_centred_and_capped() {
        let laptop = window_frame(Rect { x: 0., y: 0., width: 1512., height: 944. });
        assert_eq!((laptop.width, laptop.height), (1361., 850.));
        assert!((laptop.x + laptop.width / 2. - 756.).abs() <= 1. && (laptop.y + laptop.height / 2. - 472.).abs() <= 1.);
        let huge = window_frame(Rect { x: -1000., y: 200., width: 5120., height: 2160. });
        assert_eq!((huge.width, huge.height), REVIEW_WINDOW_MOST);
        assert!((huge.x + huge.width / 2. - 1560.).abs() <= 1. && (huge.y + huge.height / 2. - 1280.).abs() <= 1.);
    }

    // The shot zoomed in, after ReviewWindowTests' stage tests. A phone's
    // full page is 780 by 20852 pixels, 390 by 10426 points on Retina.

    fn tall() -> StageZoom {
        StageZoom::new(390., 10426., 800., 700.)
    }

    #[test]
    fn a_wheel_zooms_and_a_press_zoomed_in_drags_rather_than_going_back() {
        let mut z = tall();
        assert!(z.is_fitted());
        let fit = z.magnification;
        assert!(fit < 0.1, "the whole page fits");
        assert!(z.wheel(5., false, None));
        assert!(z.magnification > fit * 2.);
        assert!(z.is_zoomed());
        assert!(!z.press(Point { x: 400., y: 350. }, 1), "a press on a zoomed shot is for dragging");
        z.release();
        z.wheel(-40., false, None);
        assert!((z.magnification - fit).abs() < 1e-9, "it stops at the whole page");
        assert!(!z.is_zoomed());
        assert!(z.press(Point { x: 400., y: 350. }, 1), "fitted, a press is a click");
        assert!(!z.wheel(3., true, None), "Shift and a wheel scrolls sideways");
    }

    #[test]
    fn zooming_keeps_the_spot_under_the_pointer_under_it() {
        let mut z = StageZoom::new(3000., 3000., 800., 700.);
        z.apply(ZoomStep::Actual);
        let pointer = Point { x: 550., y: 250. };
        let before = z.visible();
        let spot = Point { x: before.x + pointer.x / z.magnification, y: before.y + pointer.y / z.magnification };
        z.zoom_by(1.5, Some(pointer));
        let after = z.visible();
        let now = Point { x: after.x + pointer.x / z.magnification, y: after.y + pointer.y / z.magnification };
        assert!((now.x - spot.x).abs() < 1. && (now.y - spot.y).abs() < 1.);
    }

    #[test]
    fn full_size_starts_at_the_top_in_the_middle_and_dragging_follows_the_hand() {
        let mut z = StageZoom::new(150., 10426., 800., 700.);
        z.apply(ZoomStep::Actual);
        assert!((z.magnification - 1.).abs() < 1e-9);
        let v = z.visible();
        assert_eq!(v.y, 0., "full size starts at the top of the page");
        assert!(v.x < 0., "narrower than the stage, so it's in the middle");
        assert!((v.x + v.width / 2. - 75.).abs() < 1.);
        z.press(Point { x: 400., y: 350. }, 1);
        // Up the screen: the page goes up, so further down it.
        z.drag_to(Point { x: 400., y: 150. });
        z.release();
        assert!((z.visible().y - 200.).abs() < 1.);
        assert!(!z.press(Point { x: 1., y: 1. }, 2) && z.is_fitted(), "a double click fits it");
    }

    #[test]
    fn a_swipe_zooms_from_fit_and_once_zoomed_the_next_moves_around() {
        let mut z = tall();
        let fit = z.magnification;
        z.swipe(0., 40., ScrollPhase::Started, false, None);
        z.swipe(0., 40., ScrollPhase::Moved, false, None);
        let zoomed_in = z.magnification;
        assert!(zoomed_in > fit, "nothing to scroll while it fits");
        let top = z.visible().y;
        z.swipe(0., 20., ScrollPhase::Moved, false, None);
        assert!(z.magnification > zoomed_in, "the same swipe stays a zoom right through");
        let scale = z.magnification;
        z.swipe(0., 0., ScrollPhase::Ended, false, None);
        z.swipe(0., 30., ScrollPhase::Moved, false, None);
        assert_eq!(z.magnification, scale, "its coast does nothing");
        z.swipe(0., -60., ScrollPhase::Started, false, None);
        z.swipe(0., -60., ScrollPhase::Moved, false, None);
        assert!((z.magnification - scale).abs() < 1e-9, "the next swipe moves around");
        assert_ne!(z.visible().y, top);
    }

    #[test]
    fn a_swipe_either_way_zooms_in_from_fit_and_a_held_key_zooms_the_way_it_goes() {
        let mut z = tall();
        let fit = z.magnification;
        z.swipe(0., -40., ScrollPhase::Started, false, None);
        z.swipe(0., -40., ScrollPhase::Moved, false, None);
        assert!(z.magnification > fit, "natural scrolling's swipe up");
        let mut held = tall();
        held.swipe(0., 40., ScrollPhase::Started, true, None);
        held.swipe(0., 40., ScrollPhase::Moved, true, None);
        let zoomed_in = held.magnification;
        assert!(zoomed_in > fit);
        held.swipe(0., 0., ScrollPhase::Ended, true, None);
        held.swipe(0., -20., ScrollPhase::Started, true, None);
        held.swipe(0., -20., ScrollPhase::Moved, true, None);
        assert!(held.magnification < zoomed_in, "held, it zooms even once zoomed, and the other way is out");
    }

    #[test]
    fn the_minimap_shows_the_part_on_show_and_goes_where_its_pressed() {
        let mut z = tall();
        assert!(z.minimap().is_none(), "the whole image is on show");
        z.apply(ZoomStep::Actual);
        let map = z.minimap().expect("zoomed in, it shows");
        assert_eq!(map.shown.y, 0.);
        assert!(map.frame.x + map.frame.width <= 800. - 22. + 0.5, "clear of the scroll bar down the right");
        assert_eq!(map.frame.y + map.frame.height, 700. - 16., "16 up from the bottom, as the Swift window has it");
        z.minimap_to(Point { x: map.frame.width / 2., y: map.frame.height / 2. });
        let v = z.visible();
        assert!((v.y + v.height / 2. - 10426. / 2.).abs() < 2., "halfway down the page");
        z.apply(ZoomStep::Fit);
        assert!(z.minimap().is_none());
    }

    #[test]
    fn flicking_to_todays_version_keeps_the_zoom_and_the_place() {
        let mut z = tall();
        z.apply(ZoomStep::Actual);
        z.wheel(1., false, None);
        let scale = z.magnification;
        z.go_to(0.5, 0.48);
        let before = z.visible().y;
        z.swap_image(390., 10426.);
        assert!((z.magnification - scale).abs() < 1e-9, "flicking keeps the zoom");
        assert!((z.visible().y - before).abs() < 20., "and the place");
    }

    #[test]
    fn a_small_image_fits_any_stage_and_a_resize_keeps_fitted_fitted() {
        let mut z = StageZoom::new(20., 20., 1000., 700.);
        assert!(z.fit_scale() > 4., "past the usual most");
        assert!(z.most() >= z.least());
        z.apply(ZoomStep::Closer);
        z.apply(ZoomStep::Fit);
        z.set_view(800., 660.);
        assert!(z.is_fitted());
        assert!((z.magnification - z.fit_scale()).abs() < 1e-9);
        let mut zoomed = tall();
        zoomed.apply(ZoomStep::Actual);
        zoomed.set_view(600., 500.);
        assert!((zoomed.magnification - 1.).abs() < 1e-9, "zoomed keeps its scale");
    }

    #[test]
    fn a_pinch_zooms_around_the_fingers() {
        let mut z = StageZoom::new(3000., 3000., 800., 700.);
        let fit = z.magnification;
        z.pinch(0.5, Some(Point { x: 100., y: 100. }));
        assert!(z.magnification > fit);
        z.pinch(-0.9, None);
        assert!((z.magnification - fit).abs() < 1e-9, "no further out than the whole image");
    }

    // An agent's notes.

    fn plain(text: &str) -> InlineRun {
        InlineRun { text: text.into(), ..Default::default() }
    }

    #[test]
    fn a_note_draws_its_bold_code_and_links_and_leaves_the_rest_as_written() {
        let runs = inline_markdown("Three ways to pick **which login a thread uses**. D shows it");
        assert_eq!(runs, vec![plain("Three ways to pick "), InlineRun { text: "which login a thread uses".into(), bold: true, ..Default::default() }, plain(". D shows it")]);
        let runs = inline_markdown("inside the `model` picker");
        assert_eq!(runs[1], InlineRun { text: "model".into(), code: true, ..Default::default() });
        let runs = inline_markdown("see [the *audit*](https://x.y/a) now");
        assert_eq!(runs[1], InlineRun { text: "the ".into(), link: Some("https://x.y/a".into()), ..Default::default() });
        assert_eq!(runs[2], InlineRun { text: "audit".into(), italic: true, link: Some("https://x.y/a".into()), ..Default::default() });
        assert_eq!(inline_markdown("a * b, snake_case_name"), vec![plain("a * b, snake_case_name")], "nothing that isn't emphasis");
        assert_eq!(inline_markdown("**bold** and _it_"), vec![InlineRun { text: "bold".into(), bold: true, ..Default::default() }, plain(" and "), InlineRun { text: "it".into(), italic: true, ..Default::default() }]);
        assert_eq!(inline_markdown("\\*not\\* `a*b`")[0], plain("*not* "));
        assert_eq!(inline_markdown("an [unfinished](link"), vec![plain("an [unfinished](link")]);
        assert_eq!(inline_markdown("***both***")[0], InlineRun { text: "both".into(), bold: true, italic: true, ..Default::default() });
        assert_eq!(inline_markdown("line one\n\nline two"), vec![plain("line one\n\nline two")]);
    }

    /// The parser as it was before Daybreak 93, to check the bounded one
    /// reads every short note the same.
    /// The runs a note draws as, next to each other with the same look merged.
    fn old_inline_markdown(source: &str) -> Vec<InlineRun> {
        let chars: Vec<char> = source.chars().collect();
        let mut out: Vec<InlineRun> = Vec::new();
        old_inline_runs(&chars, 0, chars.len(), InlineStyle::default(), None, &mut out);
        let mut merged: Vec<InlineRun> = Vec::new();
        for run in out.into_iter().filter(|r| !r.text.is_empty()) {
            match merged.last_mut() {
                Some(last) if (last.bold, last.italic, last.code, &last.link) == (run.bold, run.italic, run.code, &run.link) => last.text.push_str(&run.text),
                _ => merged.push(run),
            }
        }
        merged
    }

    fn old_inline_push(out: &mut Vec<InlineRun>, text: &mut String, style: InlineStyle, code: bool, link: Option<&str>) {
        if !text.is_empty() {
            out.push(InlineRun { text: std::mem::take(text), bold: style.bold, italic: style.italic, code, link: link.map(Into::into) });
        }
    }

    fn old_run_length(c: &[char], at: usize, end: usize, ch: char) -> usize {
        c[at..end].iter().take_while(|&&x| x == ch).count()
    }

    /// A delimiter run can open emphasis when what follows it isn't a space, and
    /// `_` only at the start of a word, so snake_case stays as it is.
    fn old_can_open(c: &[char], at: usize, len: usize, end: usize) -> bool {
        let next = c.get(at + len).filter(|_| at + len < end);
        let before = at.checked_sub(1).map(|i| c[i]);
        next.is_some_and(|n| !n.is_whitespace()) && (c[at] == '*' || before.is_none_or(|b| !b.is_alphanumeric()))
    }

    fn old_can_close(c: &[char], at: usize, len: usize, end: usize) -> bool {
        let before = at.checked_sub(1).map(|i| c[i]);
        let after = c.get(at + len).filter(|_| at + len < end);
        before.is_some_and(|b| !b.is_whitespace()) && (c[at] == '*' || after.is_none_or(|a| !a.is_alphanumeric()))
    }

    fn old_inline_runs(c: &[char], mut i: usize, end: usize, style: InlineStyle, link: Option<&str>, out: &mut Vec<InlineRun>) {
        let mut plain = String::new();
        while i < end {
            let ch = c[i];
            // A backslash keeps the punctuation after it as it is.
            if ch == '\\' && i + 1 < end && c[i + 1].is_ascii_punctuation() {
                plain.push(c[i + 1]);
                i += 2;
                continue;
            }
            if ch == '`' {
                let ticks = old_run_length(c, i, end, '`');
                let close = (i + ticks..end).find(|&j| c[j] == '`' && old_run_length(c, j, end, '`') == ticks && (j == 0 || c[j - 1] != '`'));
                if let Some(close) = close {
                    old_inline_push(out, &mut plain, style, false, link);
                    let mut code: String = c[i + ticks..close].iter().collect();
                    // One space either side is how a code span holds a backtick.
                    if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') {
                        code = code[1..code.len() - 1].to_owned();
                    }
                    old_inline_push(out, &mut code, style, true, link);
                    i = close + ticks;
                    continue;
                }
                plain.extend(&c[i..i + ticks]);
                i += ticks;
                continue;
            }
            if ch == '[' && link.is_none() {
                if let Some((text_end, url, after)) = old_inline_link(c, i, end) {
                    old_inline_push(out, &mut plain, style, false, link);
                    old_inline_runs(c, i + 1, text_end, style, Some(&url), out);
                    i = after;
                    continue;
                }
            }
            if (ch == '*' || ch == '_') && old_can_open(c, i, old_run_length(c, i, end, ch).min(3), end) {
                let len = old_run_length(c, i, end, ch).min(3);
                let close = (i + len..end).find(|&j| c[j] == ch && (j == 0 || c[j - 1] != ch) && old_run_length(c, j, end, ch) >= len && old_can_close(c, j, len, end));
                if let Some(close) = close {
                    old_inline_push(out, &mut plain, style, false, link);
                    let inner = InlineStyle { bold: style.bold || len >= 2, italic: style.italic || len != 2 };
                    old_inline_runs(c, i + len, close, inner, link, out);
                    i = close + len;
                    continue;
                }
            }
            plain.push(ch);
            i += 1;
        }
        old_inline_push(out, &mut plain, style, false, link);
    }

    /// `[words](address)`: where its words end, the address, and where it ends.
    fn old_inline_link(c: &[char], at: usize, end: usize) -> Option<(usize, String, usize)> {
        let mut depth = 0;
        let mut close = None;
        for j in at..end {
            match c[j] {
                '\\' => continue,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        if close + 1 >= end || c[close + 1] != '(' {
            return None;
        }
        let stop = (close + 2..end).find(|&j| c[j] == ')')?;
        let url: String = c[close + 2..stop].iter().collect::<String>().trim().to_owned();
        (!url.is_empty() && !url.contains(char::is_whitespace)).then_some((close, url, stop + 1))
    }


    /// This thread's CPU time: the M5 is often busy, so wall time would
    /// say more about the machine than the parser.
    #[cfg(unix)]
    fn thread_cpu() -> std::time::Duration {
        #[repr(C)]
        struct Timespec {
            sec: i64,
            nsec: i64,
        }
        extern "C" {
            fn clock_gettime(clock: i32, at: *mut Timespec) -> i32;
        }
        #[cfg(target_os = "macos")]
        const THREAD_CPU: i32 = 16;
        #[cfg(not(target_os = "macos"))]
        const THREAD_CPU: i32 = 3;
        let mut at = Timespec { sec: 0, nsec: 0 };
        // SAFETY: clock_gettime only writes the struct it's given.
        assert_eq!(unsafe { clock_gettime(THREAD_CPU, &mut at) }, 0);
        std::time::Duration::new(at.sec as u64, at.nsec as u32)
    }

    /// Windows has no clock_gettime (the Windows build's link failed on it,
    /// 11 Nov), so the same from GetThreadTimes: kernel and user time, in
    /// 100 ns steps. kernel32 is linked into every Windows program.
    #[cfg(windows)]
    fn thread_cpu() -> std::time::Duration {
        #[repr(C)]
        struct FileTime {
            low: u32,
            high: u32,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn GetThreadTimes(thread: *mut std::ffi::c_void, created: *mut FileTime, exited: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        }
        let zero = || FileTime { low: 0, high: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero(), zero(), zero(), zero());
        // SAFETY: GetCurrentThread's handle needs no closing, and
        // GetThreadTimes only writes the four structs it's given.
        assert_ne!(unsafe { GetThreadTimes(GetCurrentThread(), &mut created, &mut exited, &mut kernel, &mut user) }, 0);
        let ticks = |t: &FileTime| (u64::from(t.high) << 32) | u64::from(t.low);
        std::time::Duration::from_nanos((ticks(&kernel) + ticks(&user)) * 100)
    }

    fn cpu_time_of(source: &str) -> std::time::Duration {
        let start = thread_cpu();
        std::hint::black_box(inline_markdown(std::hint::black_box(source)));
        thread_cpu() - start
    }

    /// Daybreak 93's notes: backtick runs of every length from 1 up, each
    /// followed by an x, padded with backticks (20,000 characters took 7.5
    /// seconds), and the same for the other delimiters that searched to the
    /// end: unclosed emphasis, unclosed brackets, brackets with no `)`.
    /// 400 ms of CPU, not 100: a debug build on a busy Mac's efficiency cores
    /// went past 100 for linear work (DavidClarkTucson's fork, 12 Nov), and
    /// the quadratic search this guards against took seconds.
    #[test]
    fn a_crafted_note_parses_in_well_under_half_a_second() {
        let mut ticks = String::new();
        for n in 1..=90 {
            ticks.push_str(&"`".repeat(n));
            ticks.push('x');
        }
        while ticks.len() < 20_000 {
            ticks.push('`');
        }
        let notes = [
            ticks,
            "*a ".repeat(7000),
            "_a ".repeat(7000),
            "**a ".repeat(5000),
            "[".repeat(20_000),
            "[a](".repeat(5000),
            "[a]".repeat(7000),
            "*_".repeat(10_000) + "x",
            format!("{}x{}", "*a _a ".repeat(1500), " a_ a*".repeat(1500)),
            // Codex's review: runs of one delimiter, and links whose address
            // has a space up to one `)` at the end.
            "*".repeat(20_000),
            "_".repeat(20_000),
            "[a]( ".repeat(4000) + ")",
            "[a](x ".repeat(3400) + ")",
            "[a](".to_owned() + &" ".repeat(20_000) + ")",
            "[a](x".repeat(2000) + &" ".repeat(10_000) + ")",
            format!("[{}]({})", "a *b* ".repeat(3000), "u".repeat(2000)),
        ];
        for note in &notes {
            let took = cpu_time_of(note);
            assert!(took < std::time::Duration::from_millis(400), "{} characters starting {:?} took {took:?}", note.len(), &note[..12]);
        }
        // Words come back as written.
        assert_eq!(inline_markdown(&"[".repeat(20_000)), vec![plain(&"[".repeat(20_000))]);
    }

    /// A link's runs share its address rather than copying it each.
    #[test]
    fn a_links_runs_share_its_address() {
        let runs = inline_markdown(&format!("[{}]({})", "a *b* ".repeat(50), "u".repeat(2000)));
        let links: Vec<_> = runs.iter().filter_map(|r| r.link.clone()).collect();
        assert!(links.len() > 50);
        assert!(links.iter().all(|l| std::sync::Arc::ptr_eq(l, &links[0])));
    }

    /// Every short note over the delimiters reads as it did before.
    #[test]
    fn the_bounded_parser_reads_short_notes_as_the_old_one_did() {
        let alphabet: Vec<char> = "`*_[]()\\ ax".chars().collect();
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..200_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = (seed % 15) as usize;
            let mut x = seed;
            let note: String = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    alphabet[(x >> 33) as usize % alphabet.len()]
                })
                .collect();
            assert_eq!(inline_markdown(&note), old_inline_markdown(&note), "{note:?}");
        }
        for note in ["a `b` c ``d`` e", "**a _b_ c**", "[x *y*](u) `[z](w)`", "``a`b``", "*a **b** c*", "\\`x` [a]b(c)", "[a](  u  ) [b]( u v ) [c](\tw\n)", "[a]()", "[a](   )", "****a****", "___a___ __b__"] {
            assert_eq!(inline_markdown(note), old_inline_markdown(note), "{note:?}");
        }
    }

    /// A review a moved thread brought is cut to what show_options takes.
    #[test]
    fn a_reviews_words_are_cut_to_what_show_options_takes() {
        use crate::reducer::DesignReview;
        let image = serde_json::json!({ "path": "/x/a.png", "mediaType": "image/png" });
        let shot = serde_json::json!({ "title": "s".repeat(300), "image": image });
        let option = serde_json::json!({ "id": "a", "title": "t".repeat(300), "note": "n".repeat(20_000), "image": image, "shots": vec![shot; 9] });
        // As a moved thread's review.updated event could have it: the
        // other runner's word, never through the runner's cuts.
        let draft = serde_json::json!({ "pickedIds": vec!["a"; 50], "notes": (0..50).map(|n| (format!("o{n}"), serde_json::Value::from("d".repeat(20_000)))).collect::<serde_json::Map<_, _>>(), "reply": "r".repeat(1_000_000) });
        let review = serde_json::json!({ "id": "r", "title": "T".repeat(300), "note": "`".repeat(20_000), "status": "pending", "revision": 1, "options": vec![option; 13], "draft": draft, "response": draft });
        let review = DesignReview::decode(&review).expect("a review");
        assert_eq!(review.title.chars().count(), 200);
        assert_eq!(review.note.as_deref().map(|n| n.chars().count()), Some(4000));
        assert_eq!(review.options.len(), 12);
        assert!(review.options.iter().all(|o| o.note.as_deref().map(str::len) == Some(4000) && o.title.len() == 200));
        assert!(review.options.iter().all(|o| o.shots.len() == 8 && o.shots.iter().all(|s| s.title.as_deref().map(str::len) == Some(200))));
        for draft in [&review.draft, review.response.as_ref().expect("a response")] {
            assert_eq!((draft.picked_ids.len(), draft.notes.len()), (12, 12));
            assert!(draft.notes.values().all(|n| n.len() == 4000));
            assert_eq!(draft.reply.as_deref().map(str::len), Some(8000));
        }
        // Far longer lists are cut before they're decoded. Kept to small
        // shots on one axis at a time: 100,000 long-titled shots in each of
        // 20 options was about two million JSON objects, and took the cloud
        // check host down on 12 Nov.
        let small = serde_json::json!({ "image": image });
        let many_shots = serde_json::json!({ "id": "a", "title": "t", "image": image, "shots": vec![small; 10_000] });
        let long = serde_json::json!({ "id": "r", "title": "T", "status": "pending", "revision": 1, "options": [many_shots] });
        assert_eq!(DesignReview::decode(&long).expect("a review").options[0].shots.len(), 8);
        let lean = serde_json::json!({ "id": "a", "title": "t", "image": image });
        let long = serde_json::json!({ "id": "r", "title": "T", "status": "pending", "revision": 1, "options": vec![lean; 10_000] });
        assert_eq!(DesignReview::decode(&long).expect("a review").options.len(), 12);
        // Within the limits nothing changes, in characters, not bytes.
        let short = serde_json::json!({ "id": "r", "title": "é".repeat(200), "note": "ü".repeat(4000), "status": "pending", "revision": 1, "options": [] });
        let short = DesignReview::decode(&short).expect("a review");
        assert_eq!((short.title, short.note), ("é".repeat(200), Some("ü".repeat(4000))));
    }
}
