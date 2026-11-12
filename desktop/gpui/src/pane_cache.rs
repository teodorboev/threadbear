// The threads opened lately, kept loaded while another is open (the Swift app's
// `sessions`, AppModel+Selection.swift): switching back to one shows it at once,
// where it was reading, rather than reading its newest page from the runner and
// folding it again. Each stays subscribed, so it's up to date when it's shown;
// one kept across a reconnect catches up from its last seq when it's next shown
// (`ThreadPane::resubscribe`). The least recently shown goes once there are more
// than `KEEP`, and its subscription with it.
//
// A thread has one pane at a time: the main panel's, one here, or its own
// panel's (thread_panels.rs). Every way a pane moves between them goes through
// `take` and `keep`, as two panes on one thread would share its subscription,
// and the first to let go would end it for both.
use crate::thread::ThreadPane;
use gpui_kit::*;

/// Threads kept loaded, the one open in the main panel included (`maxSessions`).
pub(crate) const KEEP: usize = 24;

#[derive(Default)]
pub(crate) struct PaneCache {
    /// Least recently shown first. A thread's own pane, never a sub-agent's.
    panes: Vec<Entity<ThreadPane>>,
}

impl PaneCache {
    /// The thread's kept pane, taken out to be shown.
    pub(crate) fn take(&mut self, id: &str, cx: &App) -> Option<Entity<ThreadPane>> {
        let ix = self.panes.iter().position(|p| p.read(cx).summary.id == id)?;
        Some(self.panes.remove(ix))
    }

    /// A pane that's stopped being shown, kept as the most recent. Room is made
    /// for the one in the main panel too, so `KEEP` threads are loaded in all.
    /// A sub-agent's pane isn't kept: it's read again whole each time anyway.
    pub(crate) fn keep(&mut self, pane: Entity<ThreadPane>, cx: &mut App) {
        if pane.read(cx).subagent_id.is_some() {
            return;
        }
        let id = pane.read(cx).summary.id.clone();
        // Never two for a thread (see above): the newer one stays, and goes
        // on with the subscription they shared.
        self.forget(&id, cx);
        // Not in front of Mike, so news to it doesn't mark it read.
        pane.update(cx, |p, cx| p.set_kept(cx));
        self.panes.push(pane);
        while self.panes.len() > KEEP - 1 {
            let oldest = self.panes.remove(0);
            oldest.read(cx).unsubscribe();
        }
    }

    /// Lets a thread's pane go without a word to the runner: the thread's gone
    /// (deleted, or no longer listed), or another pane has taken it over.
    pub(crate) fn forget(&mut self, id: &str, cx: &App) {
        self.panes.retain(|p| p.read(cx).summary.id != id);
    }

    /// Lets go of every pane whose thread `keep_if` says no to, unsubscribing.
    pub(crate) fn retain(&mut self, cx: &App, mut keep_if: impl FnMut(&str) -> bool) {
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.panes).into_iter().partition(|p| keep_if(&p.read(cx).summary.id));
        for pane in gone {
            pane.read(cx).unsubscribe();
        }
        self.panes = kept;
    }

    pub(crate) fn panes(&self) -> impl Iterator<Item = &Entity<ThreadPane>> {
        self.panes.iter()
    }

    pub(crate) fn get(&self, id: &str, cx: &App) -> Option<Entity<ThreadPane>> {
        self.panes.iter().find(|p| p.read(cx).summary.id == id).cloned()
    }

    /// The kept threads, least recently shown first, for the automation's dump.
    pub(crate) fn ids(&self, cx: &App) -> Vec<String> {
        self.panes.iter().map(|p| p.read(cx).summary.id.clone()).collect()
    }
}
