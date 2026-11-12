// What the app knows about a runner restart (`runner.restart` in
// docs/protocol.md): one waiting for threads to finish, one under way, or
// none. A port of the Swift app's RunnerRestart (ThreadbearCore/RPC/
// RunnerRestart.swift). The runner only pushes `runner.notice` texts, so
// those move it along straight away, and `runner.status` (asked after each
// notice and as threads finish) says which threads it's waiting for.
//
// The connection (runner.rs) keeps one of these: it hears the notices and
// the drops first, reconnects every half second while a restart is under
// way, and holds sends for the new runner while it is.
use serde_json::Value;
use std::time::{Duration, Instant};

/// How long a restart keeps the connection banner quiet, and how long a
/// message sent while it's down waits for the new runner. A runner that
/// hasn't come back by then is down, not restarting, and gets the usual
/// banner with its Try again button (`RunnerRestart.patience`).
pub const PATIENCE: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    None,
    /// Waiting for this many threads. `thread_ids` is empty until
    /// `runner.status` answers (a notice only has the count).
    Pending { count: usize, thread_ids: Vec<String> },
    /// Shutting down, or down and about to come back.
    Restarting { since: Instant },
}

/// What a `runner.notice` says about a restart: how many threads it waits
/// for, and how many of those wait on an answer from Mike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Pending { count: usize, on_you: usize },
    Restarting,
    Cancelled,
}

/// `runner.status`'s `pendingRestart`, the parts used here.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PendingRestart {
    /// "idle" or "now".
    pub when: String,
    /// The threads it waits for. Empty means it's happening now.
    pub waiting_for: Vec<String>,
}

impl PendingRestart {
    /// Read leniently: a field an older runner leaves out is empty. None
    /// for a missing or null `pendingRestart`.
    pub fn decode(v: &Value) -> Option<PendingRestart> {
        if !v.is_object() {
            return None;
        }
        Some(PendingRestart {
            when: v["when"].as_str().unwrap_or("idle").to_owned(),
            waiting_for: v["waitingFor"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RunnerRestart {
    pub phase: Phase,
    /// From the last notice, until `runner.status` names the threads (then
    /// their statuses say which wait on Mike).
    notice_on_you: usize,
}

impl RunnerRestart {
    pub fn new(phase: Phase) -> RunnerRestart {
        RunnerRestart { phase, notice_on_you: 0 }
    }

    /// The runner's restart notices (`pendingMessage` and the cancel in
    /// runner/src/restart.ts, whose test pins these sentences), or None for
    /// any other notice. The numbers after "when" add up to the count:
    /// "when 3 threads finish", "when 1 thread finishes and you answer 2",
    /// "when you answer 1 thread".
    pub fn parse(message: &str) -> Option<Notice> {
        if message.starts_with("The runner is restarting") {
            return Some(Notice::Restarting);
        }
        if message.starts_with("The pending runner restart was cancelled") {
            return Some(Notice::Cancelled);
        }
        let rest = message.strip_prefix("The runner will restart when ")?;
        let count = rest.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<usize>().ok()).fold(0usize, usize::saturating_add);
        let on_you = rest.find("you answer ").and_then(|at| {
            let digits: String = rest[at + "you answer ".len()..].chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        });
        Some(Notice::Pending { count: count.max(1), on_you: on_you.unwrap_or(0) })
    }

    /// Returns true when the notice was about a restart, so the caller can
    /// ask `runner.status` for the details.
    pub fn notice(&mut self, message: &str, now: Instant) -> bool {
        let Some(notice) = Self::parse(message) else { return false };
        match notice {
            Notice::Pending { count, on_you } => {
                self.notice_on_you = on_you;
                // Keep the thread list from the last status if it still fits.
                self.phase = match &self.phase {
                    Phase::Pending { thread_ids, .. } if thread_ids.len() == count => Phase::Pending { count, thread_ids: thread_ids.clone() },
                    _ => Phase::Pending { count, thread_ids: vec![] },
                };
            }
            Notice::Restarting => {
                if !matches!(self.phase, Phase::Restarting { .. }) {
                    self.phase = Phase::Restarting { since: now };
                }
            }
            Notice::Cancelled => self.phase = Phase::None,
        }
        true
    }

    /// A `runner.status` answer while connected.
    pub fn status(&mut self, pending: Option<&PendingRestart>, now: Instant) {
        let Some(pending) = pending else {
            // Nothing pending any more (it was cancelled). One already under
            // way stays so until the connection drops: a status sent just
            // before the shutdown can't undo it.
            if matches!(self.phase, Phase::Pending { .. }) {
                self.phase = Phase::None;
            }
            return;
        };
        if matches!(self.phase, Phase::Restarting { .. }) {
            return;
        }
        if pending.when == "now" || pending.waiting_for.is_empty() {
            self.phase = Phase::Restarting { since: now };
        } else {
            self.phase = Phase::Pending { count: pending.waiting_for.len(), thread_ids: pending.waiting_for.clone() };
        }
    }

    /// The connection dropped, or an attempt to connect failed. A pending
    /// restart that loses the connection has begun: the "restarting now"
    /// notice can go down with the socket.
    pub fn disconnected(&mut self, now: Instant) {
        if matches!(self.phase, Phase::Pending { .. }) {
            self.phase = Phase::Restarting { since: now };
        }
    }

    /// Connected again: the restart is over. A new one that's pending comes
    /// back with hello's notice and `runner.status`.
    pub fn connected(&mut self) {
        self.phase = Phase::None;
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.phase, Phase::Pending { .. })
    }

    /// Under way, and not for so long that the runner looks simply down.
    pub fn is_restarting(&self, now: Instant) -> bool {
        matches!(self.phase, Phase::Restarting { since } if now.saturating_duration_since(since) < PATIENCE)
    }

    /// Pending, or under way and not so long that the runner looks simply
    /// down (`AppModel.restartUnderWay`).
    pub fn under_way(&self, now: Instant) -> bool {
        self.is_pending() || self.is_restarting(now)
    }

    /// When the restart under way began, for working out when it stops
    /// being one.
    pub fn restarting_since(&self) -> Option<Instant> {
        match self.phase {
            Phase::Restarting { since } => Some(since),
            _ => None,
        }
    }

    /// The pending restart in the runner's own words, counting the threads
    /// `waiting_on_you` picks (waiting on an approval or a question) apart:
    /// those won't finish until Mike answers.
    pub fn pending_text(&self, waiting_on_you: impl Fn(&str) -> bool) -> Option<String> {
        let Phase::Pending { count, thread_ids } = &self.phase else { return None };
        let on_you = if thread_ids.is_empty() { self.notice_on_you.min(*count) } else { thread_ids.iter().filter(|id| waiting_on_you(id)).count() };
        Some(Self::text(*count, on_you))
    }

    /// `pendingMessage` in runner/src/restart.ts.
    pub fn text(waiting: usize, on_you: usize) -> String {
        let answer = on_you.min(waiting);
        let finishing = waiting - answer;
        let finish = if finishing == 1 { "1 thread finishes".to_owned() } else { format!("{finishing} threads finish") };
        if answer == 0 {
            return format!("The runner will restart when {finish}.");
        }
        if finishing == 0 {
            return format!("The runner will restart when you answer {}.", if answer == 1 { "1 thread".to_owned() } else { format!("{answer} threads") });
        }
        format!("The runner will restart when {finish} and you answer {answer}.")
    }

    /// The threads it's waiting for, by title, for the banner's tooltip.
    pub fn waiting_for_text(&self, title: impl Fn(&str) -> Option<String>) -> Option<String> {
        let Phase::Pending { thread_ids, .. } = &self.phase else { return None };
        if thread_ids.is_empty() {
            return None;
        }
        let names: Vec<String> = thread_ids.iter().map(|id| title(id).filter(|t| !t.is_empty()).unwrap_or_else(|| id.clone())).collect();
        Some(format!("Waiting for {}", names.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(waiting_for: &[&str]) -> PendingRestart {
        PendingRestart { when: "idle".into(), waiting_for: waiting_for.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn parses_the_runners_notices() {
        let p = |count, on_you| Some(Notice::Pending { count, on_you });
        assert_eq!(RunnerRestart::parse("The runner will restart when 2 threads finish."), p(2, 0));
        assert_eq!(RunnerRestart::parse("The runner will restart when 1 thread finishes."), p(1, 0));
        assert_eq!(RunnerRestart::parse("The runner will restart when 1 thread finishes and you answer 2."), p(3, 2));
        assert_eq!(RunnerRestart::parse("The runner will restart when you answer 2 threads."), p(2, 2));
        assert_eq!(RunnerRestart::parse("The runner is restarting now."), Some(Notice::Restarting));
        assert_eq!(RunnerRestart::parse("The pending runner restart was cancelled."), Some(Notice::Cancelled));
        assert_eq!(RunnerRestart::parse("Release X kept crashing at startup, so the runner went back to Y."), None);
        assert_eq!(RunnerRestart::parse("claude isn't on PATH"), None);
    }

    #[test]
    fn pending_then_status_then_restart_then_back() {
        let now = Instant::now();
        let mut restart = RunnerRestart::default();
        assert!(restart.notice("The runner will restart when 2 threads finish.", now));
        assert_eq!(restart.pending_text(|_| false).as_deref(), Some("The runner will restart when 2 threads finish."));
        // The notice has the count; runner.status names the threads.
        restart.status(Some(&pending(&["a", "b"])), now);
        assert_eq!(restart.phase, Phase::Pending { count: 2, thread_ids: vec!["a".into(), "b".into()] });
        assert_eq!(restart.waiting_for_text(|id| (id == "a").then(|| "Fix the build".to_owned())).as_deref(), Some("Waiting for Fix the build, b"));
        // One finishes.
        restart.status(Some(&pending(&["b"])), now);
        assert_eq!(restart.pending_text(|_| false).as_deref(), Some("The runner will restart when 1 thread finishes."));
        assert!(restart.notice("The runner is restarting now.", now));
        assert!(restart.is_restarting(now + Duration::from_secs(5)));
        assert_eq!(restart.pending_text(|_| false), None);
        restart.disconnected(now + Duration::from_secs(1));
        assert_eq!(restart.phase, Phase::Restarting { since: now }, "losing the connection keeps when it started");
        restart.connected();
        assert_eq!(restart.phase, Phase::None);
    }

    /// Threads waiting on Mike's answer won't finish by themselves, so the
    /// line says he's what it waits for (the runner's own sentences).
    #[test]
    fn threads_waiting_on_you_are_counted_apart() {
        let now = Instant::now();
        let mut restart = RunnerRestart::default();
        assert!(restart.notice("The runner will restart when 1 thread finishes and you answer 2.", now));
        assert_eq!(restart.pending_text(|_| false).as_deref(), Some("The runner will restart when 1 thread finishes and you answer 2."));
        restart.status(Some(&pending(&["a", "b", "c"])), now);
        assert_eq!(restart.pending_text(|id| id != "a").as_deref(), Some("The runner will restart when 1 thread finishes and you answer 2."));
        // Mike answers one; it's running again until it finishes.
        assert_eq!(restart.pending_text(|id| id == "c").as_deref(), Some("The runner will restart when 2 threads finish and you answer 1."));
        restart.status(Some(&pending(&["c"])), now);
        assert_eq!(restart.pending_text(|id| id == "c").as_deref(), Some("The runner will restart when you answer 1 thread."));
        assert_eq!(RunnerRestart::text(2, 2), "The runner will restart when you answer 2 threads.");
        assert_eq!(RunnerRestart::text(3, 0), "The runner will restart when 3 threads finish.");
    }

    #[test]
    fn other_notices_change_nothing() {
        let mut restart = RunnerRestart::default();
        assert!(!restart.notice("Release X kept crashing at startup, so the runner went back to Y.", Instant::now()));
        assert_eq!(restart.phase, Phase::None);
    }

    #[test]
    fn losing_the_connection_while_pending_means_it_began() {
        let now = Instant::now();
        // The "restarting now" notice can go down with the socket.
        let mut restart = RunnerRestart::new(Phase::Pending { count: 1, thread_ids: vec!["a".into()] });
        restart.disconnected(now);
        assert!(restart.is_restarting(now));
        // A plain drop with nothing pending is just a drop.
        let mut quiet = RunnerRestart::default();
        quiet.disconnected(now);
        assert_eq!(quiet.phase, Phase::None);
    }

    #[test]
    fn a_runner_that_doesnt_come_back_is_down_not_restarting() {
        let now = Instant::now();
        let restart = RunnerRestart::new(Phase::Restarting { since: now });
        assert!(restart.is_restarting(now + PATIENCE - Duration::from_secs(1)));
        assert!(!restart.is_restarting(now + PATIENCE + Duration::from_secs(1)));
        assert!(!restart.under_way(now + PATIENCE + Duration::from_secs(1)));
    }

    #[test]
    fn cancel_and_an_empty_status_clear_a_pending_restart() {
        let now = Instant::now();
        let mut restart = RunnerRestart::new(Phase::Pending { count: 2, thread_ids: vec![] });
        restart.notice("The pending runner restart was cancelled.", now);
        assert_eq!(restart.phase, Phase::None);
        let mut restart = RunnerRestart::new(Phase::Pending { count: 2, thread_ids: vec![] });
        restart.status(None, now);
        assert_eq!(restart.phase, Phase::None);
    }

    #[test]
    fn a_status_sent_just_before_the_shutdown_cant_undo_it() {
        let now = Instant::now();
        let mut restart = RunnerRestart::new(Phase::Restarting { since: now });
        restart.status(None, now + Duration::from_secs(1));
        assert_eq!(restart.phase, Phase::Restarting { since: now });
        restart.status(Some(&pending(&["a"])), now + Duration::from_secs(1));
        assert_eq!(restart.phase, Phase::Restarting { since: now });
    }

    #[test]
    fn status_with_nobody_to_wait_for_or_now_is_under_way() {
        let now = Instant::now();
        let mut restart = RunnerRestart::default();
        restart.status(Some(&pending(&[])), now);
        assert!(restart.is_restarting(now));
        let mut now_restart = RunnerRestart::default();
        now_restart.status(Some(&PendingRestart { when: "now".into(), waiting_for: vec!["a".into()] }), now);
        assert!(now_restart.is_restarting(now));
    }

    #[test]
    fn pending_restart_decodes_leniently() {
        let v: Value = serde_json::from_str(r#"{"when":"idle","requestedAt":"2026-10-27T10:00:00Z","waitingFor":["a","b"],"resumeThreadId":"c"}"#).unwrap();
        assert_eq!(PendingRestart::decode(&v), Some(pending(&["a", "b"])));
        assert_eq!(PendingRestart::decode(&Value::Null), None);
        assert_eq!(PendingRestart::decode(&serde_json::json!({})), Some(pending(&[])));
    }
}
