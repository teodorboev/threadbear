// The connection to the runner: JSON-RPC 2.0 over its WebSocket
// (docs/protocol.md). It signs in with the challenge (auth.rs), says hello,
// passes requests through and hands notifications on, and reconnects by
// itself when the runner restarts. The interface only ever sees results
// and notifications, never the token.
use crate::auth::{self, Role};
use crate::config::RunnerConfig;
use crate::log;
use crate::restart::{PendingRestart, RunnerRestart, PATIENCE};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;

const CLIENT_NAME: &str = "threadbear-desktop";
/// The longest wait between tries to reach the runner.
pub const MAX_RETRY_MS: u64 = 2_000;
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a request waits for its answer (`RunnerAPI`'s timeouts). Two
/// minutes for most; longer for the ones the runner takes its time over, so
/// a chip or a sheet doesn't say it failed while the runner's still at it.
/// A command chip answers when its command has finished: Land has an hour
/// for queueing and checking, other commands ten minutes, and each gets a
/// minute more for the reply (`runAction`).
pub fn timeout_for(method: &str, params: &Value) -> Duration {
    match method {
        "actions.run" if params["actionId"] == "worktree.land" => Duration::from_secs(61 * 60),
        "actions.run" => Duration::from_secs(11 * 60),
        // A slow parse of "when" on the pool's CLI, a Convex CLI's first key.
        "threads.hibernateWhen" | "panels.convexDeployments" | "panels.convexKey" => Duration::from_secs(150),
        _ => Duration::from_secs(120),
    }
}

/// The codes of the errors made up on this side, never by the runner.
/// There was no connection, so it never went.
pub const NOT_CONNECTED: i64 = -1;
/// The connection closed with it on its way: it may or may not have arrived.
pub const CLOSED: i64 = -2;
/// No answer in time (`timeout_for`).
pub const TIMED_OUT: i64 = -3;
/// The runner's: no such thread (`RPCError.threadNotFound`).
pub const THREAD_NOT_FOUND: i64 = -32004;
/// The runner's: busy, or handing over to a new one (`RPCError.busy`).
pub const BUSY: i64 = -32020;

/// What the app was built from, for hello's `build` (docs/protocol.md, "App
/// restarts"): set once at launch by the installed app, so the runner can
/// tell a thread waiting for the app to restart when it's back on a new
/// build. Any other launch sends none.
static HELLO_BUILD: std::sync::OnceLock<Value> = std::sync::OnceLock::new();

pub fn set_hello_build(build: Value) {
    let _ = HELLO_BUILD.set(build);
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    Connecting { url: String },
    #[serde(rename_all = "camelCase")]
    Connected { url: String, hello: Value },
    #[serde(rename_all = "camelCase")]
    Disconnected { url: String, reason: String, retry_in_ms: u64, kind: Disconnect },
}

/// Why it isn't connected, as the Swift app tells them apart for its banner
/// (`AppModel.Connection`): it was connected and the connection went
/// (`lost`), the runner turned the token down (`unauthorized`), or the
/// runner couldn't be reached at all (`runnerDown`), which is when Start
/// runner can help.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Disconnect {
    Lost,
    Refused,
    Down,
}

/// Why it couldn't connect, in plain words, for the banner's tooltip: the
/// raw reason (with its ws:// address, "IO error" and "os error 61") is for
/// the log (`log.rs`), not for Mike.
pub fn plain_reason(reason: &str) -> String {
    let lower = reason.to_lowercase();
    if lower.contains("connection refused") {
        return "Nothing is answering on the runner's port, so it isn't running.".into();
    }
    if lower.contains("timed out") {
        return "The runner didn't answer in time.".into();
    }
    if reason.starts_with(TURNED_DOWN) || reason.starts_with("The runner") || reason.starts_with("Whatever is on") {
        return reason.to_owned();
    }
    "Something went wrong reaching it. The app's log has the details.".into()
}

/// How `session` says the runner turned down the sign-in, so `run` can tell
/// that apart from not reaching it.
const TURNED_DOWN: &str = "The runner turned down the sign-in";

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    fn local(code: i64, message: impl Into<String>) -> Self {
        RpcError { code, message: message.into(), data: None }
    }

    fn not_connected() -> Self {
        RpcError::local(NOT_CONNECTED, "Not connected to the runner")
    }

    fn closed() -> Self {
        RpcError::local(CLOSED, "The connection to the runner closed")
    }

    /// Made up on this side: no connection, it closed, or no answer in time.
    pub fn is_connection(&self) -> bool {
        matches!(self.code, NOT_CONNECTED | CLOSED | TIMED_OUT)
    }

    /// A send (or `threads.update`) the restarting runner refused before
    /// taking it, so sending again can't make two: "Thread not found" from
    /// a runner that has let the idle thread go, or its handing-over
    /// refusal (runner.ts `send` and `mustGet`, whose words these are). Or
    /// one that never left, as the connection had gone. Not a dropped
    /// connection, which might have taken the message first
    /// (`AppModel.turnedAwayByRestart`).
    pub fn turned_away_by_restart(&self) -> bool {
        match self.code {
            THREAD_NOT_FOUND | NOT_CONNECTED => true,
            // A handover's refusal, or a restart's own once it has begun
            // ("The runner is restarting; send again in a moment.",
            // restart.ts `begin`): both before anything was taken.
            BUSY => self.message.starts_with("The runner is handing over to a new one") || self.message.starts_with("The runner is restarting"),
            _ => false,
        }
    }
}

/// How a request held for a restarting runner is getting on, for its
/// bubble: waiting for the new runner, or going now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Held,
    Sending,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

/// A message cut off as the runner restarted (`send_through_restart`): how
/// to tell whether it had arrived after all, before sending it again.
pub struct Resend {
    pub thread_id: String,
    /// The thread's last seq when it was sent: only an event after it can
    /// be this message.
    pub after_seq: i64,
    /// The thread was idle and this the only message on its way to it, so
    /// nothing on the runner could have queued it: it either started a turn
    /// (its user.message is in the thread) or never got there. A busy
    /// thread's runner may hold it in a queue no one can see, so that one
    /// isn't sent twice on a guess.
    pub was_idle: bool,
    /// Whether an event (as the runner sends it) is this message landing.
    pub landed: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
}

/// The order of the requests about each of this runner's threads: each
/// has a place from when it's made (`Client::place`) until it's answered,
/// and goes once every one made before it for the same thread has been, so
/// a pick made first lands first, and one the restarting runner turns away
/// goes again before anything made after it. A place is taken where the
/// request is made, on the app's thread, rather than once its task first
/// runs: two tasks started one after the other can run in either order
/// (Codex's review, 12 Nov). Per thread, so one thread's slow answer never
/// holds up another's.
#[derive(Default)]
struct Order {
    places: Mutex<BTreeMap<u64, String>>,
    next: AtomicU64,
    turn: tokio::sync::Notify,
}

/// A request's place in its thread's order (`Order`). Dropping it gives it
/// up.
pub struct Place {
    id: u64,
    thread: String,
    order: Arc<Order>,
}

impl Place {
    /// Nothing made before this one for its thread is still waiting or
    /// going.
    fn is_first(&self) -> bool {
        !self.order.places.lock().unwrap().range(..self.id).any(|(_, t)| *t == self.thread)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.order.places.lock().unwrap().remove(&self.id);
        self.order.turn.notify_waiters();
    }
}

/// Where status changes and notifications go: the app's event channel, or a
/// channel in tests.
pub trait Sink: Send + Sync + 'static {
    fn status(&self, status: &Status);
    fn notification(&self, method: &str, params: &Value);
}

type Pending = HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>;

pub struct Client {
    config: RunnerConfig,
    sink: Box<dyn Sink>,
    out: Mutex<Option<mpsc::UnboundedSender<Message>>>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    status: Mutex<Status>,
    /// Wakes the reconnect wait when someone presses Try again.
    retry: tokio::sync::Notify,
    /// What the runner has said about restarting (restart.rs).
    restart: Mutex<RunnerRestart>,
    /// The connection that's signed in and said hello, by a number that's
    /// new each time, or 0 while there's none: what a held request waits on.
    session: watch::Sender<u64>,
    sessions: AtomicU64,
    /// Requests for this runner's threads, in the order they were made
    /// (`restartQueue`).
    order: Arc<Order>,
    /// Set as the app quits (`stop_holding`): a held request no longer
    /// goes, so one kept in the drafts file can't also have been sent.
    holding_stopped: Mutex<bool>,
    /// How long a message sent while the runner's down waits for it, and
    /// one a restart turned away waits for the new runner. Only tests
    /// change them.
    waits: Mutex<(Duration, Duration)>,
    /// The last reason it couldn't connect, so the log says each once.
    last_down: Mutex<Option<String>>,
}

impl Client {
    pub fn new(config: RunnerConfig, sink: impl Sink) -> Arc<Self> {
        let url = config.url();
        Arc::new(Client {
            config,
            sink: Box::new(sink),
            out: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            // 1 is auth.prove's.
            next_id: AtomicU64::new(2),
            status: Mutex::new(Status::Connecting { url }),
            retry: tokio::sync::Notify::new(),
            restart: Mutex::new(RunnerRestart::default()),
            session: watch::channel(0).0,
            sessions: AtomicU64::new(0),
            order: Arc::new(Order::default()),
            holding_stopped: Mutex::new(false),
            waits: Mutex::new((PATIENCE, Duration::from_secs(30))),
            last_down: Mutex::new(None),
        })
    }

    /// What the runner has said about restarting, as of now.
    pub fn restart(&self) -> RunnerRestart {
        self.restart.lock().unwrap().clone()
    }

    /// A restart is on: pending, or under way and not so long that the
    /// runner looks simply down (`restartUnderWay`).
    pub fn restart_under_way(&self) -> bool {
        self.restart.lock().unwrap().under_way(Instant::now())
    }

    /// `runner.status`'s `pendingRestart`, which names the threads a
    /// restart waits for.
    pub fn restart_status(&self, pending_restart: &Value) {
        let pending = PendingRestart::decode(pending_restart);
        self.update_restart(|r| r.status(pending.as_ref(), Instant::now()));
    }

    fn update_restart(&self, change: impl FnOnce(&mut RunnerRestart)) {
        let mut restart = self.restart.lock().unwrap();
        let before = restart.clone();
        change(&mut restart);
        if *restart == before {
            return;
        }
        if restart.restarting_since().is_some() && before.restarting_since().is_none() {
            log::write("restart", "the runner is restarting");
        } else if let Some(text) = restart.pending_text(|_| false).filter(|t| Some(t) != before.pending_text(|_| false).as_ref()) {
            log::write("restart", &text);
        }
    }

    /// The connection signed in now, if there is one.
    pub fn connected_session(&self) -> Option<u64> {
        Some(*self.session.borrow()).filter(|s| *s != 0)
    }

    /// For tests: how long a held message waits for the runner to come
    /// back, and one a restart turned away waits for the new runner.
    pub fn set_restart_waits(&self, hold: Duration, resend: Duration) {
        *self.waits.lock().unwrap() = (hold, resend);
    }

    /// "Try again" in the connection banner: stop waiting and reconnect now.
    pub fn retry_now(&self) {
        self.retry.notify_one();
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, status: Status) {
        *self.status.lock().unwrap() = status.clone();
        self.sink.status(&status);
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let result = self.call_once(method, params).await;
        if let Err(e) = &result {
            // The runner's words or the connection's, never what was sent.
            log::write("rpc", &format!("{method} failed ({}): {}", e.code, e.message));
        }
        result
    }

    async fn call_once(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let wait = timeout_for(method, &params);
        let text = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        {
            let out = self.out.lock().unwrap();
            let Some(out) = out.as_ref() else {
                return Err(RpcError::not_connected());
            };
            self.pending.lock().unwrap().insert(id, tx);
            if out.send(Message::text(text)).is_err() {
                self.pending.lock().unwrap().remove(&id);
                return Err(RpcError::not_connected());
            }
        }
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcError::closed()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                let minutes = wait.as_secs() / 60;
                let took = if wait.as_secs() >= 600 { format!("{minutes} minutes") } else { format!("{} seconds", wait.as_secs()) };
                Err(RpcError::local(TIMED_OUT, format!("The runner didn't answer {method} in {took}")))
            }
        }
    }

    /// A request about one of this runner's threads (a message, a card's
    /// answers, a rename or an archive), as `inRestartOrder` and
    /// `whenRestarted` send it. While the runner restarts it waits for the
    /// new one rather than failing; one the old runner turns away goes
    /// again once the new one is up; and while any is waiting, later ones
    /// go behind it, so everything lands in the order Mike made it: a pick
    /// made once the new runner is up can't be undone by an older one
    /// going again after it, and a message can't start its turn before the
    /// model he picked first. `progress` hears when it's held and when it
    /// goes.
    pub async fn call_in_restart_order(&self, method: &str, params: Value, progress: ProgressFn) -> Result<Value, RpcError> {
        let place = self.place(params["threadId"].as_str().unwrap_or(""));
        self.call_in_restart_order_at(place, method, params, progress).await
    }

    /// `call_in_restart_order` from a place taken when the request was made.
    pub async fn call_in_restart_order_at(&self, place: Place, method: &str, params: Value, progress: ProgressFn) -> Result<Value, RpcError> {
        self.in_restart_order(place, method, params, &progress, None).await.map(|v| v.unwrap_or(Value::Null))
    }

    /// A place in the order of requests about this runner's threads, to
    /// take where the request is made (`Order`) and hand to
    /// `send_through_restart` or `call_in_restart_order_at`.
    pub fn place(&self, thread_id: &str) -> Place {
        let id = self.order.next.fetch_add(1, Ordering::SeqCst);
        self.order.places.lock().unwrap().insert(id, thread_id.to_owned());
        Place { id, thread: thread_id.to_owned(), order: self.order.clone() }
    }

    /// As the app quits: requests still held for a restarting runner stay
    /// held, as their words go in the drafts file. One that has already said
    /// it's going (`Progress::Sending`) goes.
    pub fn stop_holding(&self) {
        *self.holding_stopped.lock().unwrap() = true;
    }

    /// A held request about to go: it says so, unless the app is quitting.
    /// The two under one lock, so after `stop_holding` returns every held
    /// request has either said it's going or never will.
    fn go(&self, progress: &ProgressFn) -> Result<(), RpcError> {
        let stopped = self.holding_stopped.lock().unwrap();
        if *stopped {
            return Err(RpcError::local(NOT_CONNECTED, "The app is quitting"));
        }
        progress(Progress::Sending);
        Ok(())
    }

    /// A message to one of this runner's threads (`AppModel.post`):
    /// `call_in_restart_order`, and one cut off on its way as a restarting
    /// runner closed its port, so no answer came, waits for the new runner
    /// and goes again there, unless the thread shows it arrived. Only for an
    /// idle thread with nothing else on its way to it (`Resend::was_idle`).
    /// Ok(None) when it had arrived after all.
    pub async fn send_through_restart(&self, place: Place, params: Value, resend: Resend, progress: ProgressFn) -> Result<Option<Value>, RpcError> {
        self.in_restart_order(place, "threads.send", params, &progress, Some(&resend)).await
    }

    async fn in_restart_order(&self, place: Place, method: &str, params: Value, progress: &ProgressFn, resend: Option<&Resend>) -> Result<Option<Value>, RpcError> {
        let sent_down = self.connected_session().is_none();
        if sent_down && !self.restart_under_way() {
            return Err(RpcError::not_connected());
        }
        if sent_down {
            // Sent while the runner's down: it waits for the new one.
            log::write("restart", &format!("holding {method} for the new runner"));
            progress(Progress::Held);
        } else if !place.is_first() && self.restart_under_way() {
            // Behind one the restart is holding. Behind one that's only on
            // its way, it waits a moment and says nothing.
            progress(Progress::Held);
        }
        // Its place is kept until it's answered, through a second try on
        // the new runner, so nothing made after it gets there first.
        self.behind_restart(place, async {
            let now = match self.connected_session() {
                Some(now) => now,
                // Down when it was made, or gone while it waited its turn:
                // the new runner, while a restart's under way.
                None if self.restart_under_way() => {
                    let (hold, resend_wait) = *self.waits.lock().unwrap();
                    let Some(now) = self.wait_for_session(None, if sent_down { hold } else { resend_wait }).await else {
                        return Err(if sent_down { RpcError::local(NOT_CONNECTED, "The runner didn't come back") } else { RpcError::not_connected() });
                    };
                    now
                }
                None => return Err(RpcError::not_connected()),
            };
            self.go(progress)?;
            let first = self.call(method, params.clone()).await;
            self.settle(first, now, method, &params, progress, resend).await
        })
        .await
    }

    /// A message cut off on its way that may have never arrived, to an idle
    /// thread with nothing else on its way (`Resend::was_idle`).
    fn may_resend(error: &RpcError, method: &str, resend: Option<&Resend>) -> bool {
        error.code == CLOSED && method == "threads.send" && resend.is_some_and(|r| r.was_idle)
    }

    /// What's done with a request's first answer, inside its turn in the
    /// restart queue, so nothing behind it overtakes it while it goes again:
    /// one turned away goes again on the new runner (`againAfterRestart`),
    /// and a message cut off on its way goes again unless it arrived.
    async fn settle(&self, first: Result<Value, RpcError>, sent_on: u64, method: &str, params: &Value, progress: &ProgressFn, resend: Option<&Resend>) -> Result<Option<Value>, RpcError> {
        let mut result = first;
        if let Err(e) = &result {
            if e.turned_away_by_restart() && self.restart_under_way() {
                result = self.again_after_restart(e.clone(), sent_on, method, params, progress).await;
            }
        }
        match result {
            Err(e) if Self::may_resend(&e, method, resend) && self.restart_under_way() => {
                progress(Progress::Held);
                self.resend_cut_off(e, params.clone(), resend.expect("may_resend has one"), progress).await
            }
            other => other.map(Some),
        }
    }

    async fn resend_cut_off(&self, error: RpcError, params: Value, resend: &Resend, progress: &ProgressFn) -> Result<Option<Value>, RpcError> {
        let id = resend.thread_id.as_str();
        // The connection it was cut off on is gone, so any that's up now is
        // the new runner's (`run` clears it before failing what was on its way).
        let hold = self.waits.lock().unwrap().0;
        if self.wait_for_session(None, hold).await.is_none() {
            return Err(error);
        }
        // Still with the old runner (finishing a turn-end hook, say) or
        // busy: it may sit in a queue there that nothing shows, so back in
        // the box rather than go twice.
        let list = self.call("threads.list", json!({ "include": ["inbox", "snoozed", "archived"], "ids": [id] })).await?;
        let summary = list["threads"].as_array().into_iter().flatten().find(|t| t["id"] == id).cloned().unwrap_or(Value::Null);
        let busy = matches!(summary["status"].as_str(), Some("running" | "starting" | "waiting"));
        if summary.is_null() || summary["onOldRunner"] == true || busy {
            log::write("send", &format!("a send the restart cut off may be queued on the old runner: {id}"));
            return Err(error);
        }
        let last_seq = summary["lastSeq"].as_i64().unwrap_or(0);
        // Every page since it was sent, not just the first.
        let mut after = resend.after_seq;
        for _ in 0..20 {
            let page = self.call("threads.events", json!({ "threadId": id, "afterSeq": after })).await?;
            let events = page["events"].as_array().cloned().unwrap_or_default();
            if events.iter().any(|e| (resend.landed)(e)) {
                log::write("send", &format!("a send the restart cut off had arrived: {id}"));
                return Ok(None);
            }
            match events.last().and_then(|e| e["seq"].as_i64()) {
                Some(last) if last > after && last < last_seq => after = last,
                _ => break,
            }
        }
        log::write("send", &format!("sending again after the restart cut it off: {id}"));
        self.go(progress)?;
        self.call("threads.send", params).await.map(Some)
    }

    /// `request` once more on the new runner, if `error` is a restart's
    /// refusal and the new runner connects within the resend wait;
    /// otherwise `error`. Inside the queue's step, never through it again,
    /// which would wait on itself (`againAfterRestart`).
    async fn again_after_restart(&self, error: RpcError, sent_on: u64, method: &str, params: &Value, progress: &ProgressFn) -> Result<Value, RpcError> {
        if !error.turned_away_by_restart() || !self.restart_under_way() {
            return Err(error);
        }
        progress(Progress::Held);
        let resend = self.waits.lock().unwrap().1;
        if self.wait_for_session(Some(sent_on), resend).await.is_none() {
            return Err(error);
        }
        log::write("restart", &format!("sending {method} again after the restart"));
        self.go(progress)?;
        self.call(method, params.clone()).await
    }

    /// Runs `step` once every request made before `place` has gone
    /// (`behindRestart`), then gives the place up.
    async fn behind_restart<T>(&self, place: Place, step: impl Future<Output = T>) -> T {
        loop {
            // Listening before looking, so a place given up in between
            // still wakes it.
            let turn = self.order.turn.notified();
            let mut turn = std::pin::pin!(turn);
            turn.as_mut().enable();
            if place.is_first() {
                break;
            }
            turn.await;
        }
        let out = step.await;
        drop(place);
        out
    }

    /// A connection that's signed in and isn't `old`, within `wait`
    /// (`clientAfterRestart`).
    async fn wait_for_session(&self, old: Option<u64>, wait: Duration) -> Option<u64> {
        let mut rx = self.session.subscribe();
        let until = async {
            loop {
                let now = *rx.borrow_and_update();
                if now != 0 && Some(now) != old {
                    return now;
                }
                if rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::time::timeout(wait, until).await.ok()
    }

    /// Connects, and connects again whenever the connection drops, for as
    /// long as the app runs.
    pub async fn run(self: Arc<Self>) {
        let mut backoff_ms = 500;
        loop {
            let url = self.config.url();
            self.set_status(Status::Connecting { url: url.clone() });
            let (reason, kind) = match self.clone().session(&url).await {
                Ok(()) => {
                    backoff_ms = 500;
                    log::write("connection", "lost the connection to the runner");
                    ("The runner closed the connection".to_string(), Disconnect::Lost)
                }
                Err(e) if e.starts_with(TURNED_DOWN) => {
                    log::write("connection", &e);
                    (e, Disconnect::Refused)
                }
                Err(e) => {
                    let mut last = self.last_down.lock().unwrap();
                    if last.as_deref() != Some(e.as_str()) {
                        log::write("connection", &format!("can't reach the runner: {e}"));
                        *last = Some(e.clone());
                    }
                    (e, Disconnect::Down)
                }
            };
            self.out.lock().unwrap().take();
            // Gone before what was on its way hears so, so a request cut off
            // can tell any connection after this from the one it was on.
            self.session.send_replace(0);
            for (_, tx) in self.pending.lock().unwrap().drain() {
                let _ = tx.send(Err(RpcError::closed()));
            }
            self.update_restart(|r| r.disconnected(Instant::now()));
            // A restart is over in a few seconds; backing off to 4 s and more
            // left the window saying "Reconnecting" well after the runner
            // was back (`connectLoop`).
            let wait_ms = if self.restart.lock().unwrap().is_restarting(Instant::now()) { backoff_ms.min(500) } else { backoff_ms };
            self.set_status(Status::Disconnected { url, reason, retry_in_ms: wait_ms, kind });
            // "Try again" in the connection banner cuts the wait short.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {}
                _ = self.retry.notified() => {}
            }
            // This Mac's own runner: a refused connection costs nothing, and
            // a runner that's back shouldn't wait out a long backoff (Mike,
            // 12 Nov, a "Trying again in 8s" in the middle of a restart).
            backoff_ms = (backoff_ms * 2).min(MAX_RETRY_MS);
        }
    }

    /// One connection, from dialing to closing. Ok once it was signed in
    /// and then closed; Err with what went wrong before that.
    async fn session(self: Arc<Self>, url: &str) -> Result<(), String> {
        let token = self.config.read_token()?;
        let (ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| format!("Couldn't reach the runner at {url}: {e}"))?;
        let (mut write, mut read) = ws.split();

        // The challenge, our proof, and the runner's proof back.
        let challenge = tokio::time::timeout(AUTH_TIMEOUT, next_json(&mut read))
            .await
            .map_err(|_| "The runner didn't send an auth challenge".to_string())??;
        let runner_nonce = challenge
            .pointer("/params/nonce")
            .and_then(Value::as_str)
            .filter(|n| challenge.get("method").and_then(Value::as_str) == Some("auth.challenge") && auth::is_nonce(n))
            .ok_or("The runner didn't send an auth challenge, so it isn't the runner or it's older than this app")?
            .to_string();
        // A loopback client binds its proof to the port it dialed.
        let port = self.config.port;
        let client_nonce = auth::new_nonce();
        let prove = json!({
            "jsonrpc": "2.0", "id": 1, "method": "auth.prove",
            "params": { "nonce": client_nonce, "proof": auth::proof(&token, Role::Client, &runner_nonce, &client_nonce, "primary", port) }
        });
        write.send(Message::text(prove.to_string())).await.map_err(|e| e.to_string())?;
        let answer = tokio::time::timeout(AUTH_TIMEOUT, next_json(&mut read))
            .await
            .map_err(|_| "The runner didn't answer the sign-in".to_string())??;
        if let Some(err) = answer.get("error") {
            return Err(format!("{TURNED_DOWN}: {}", err.get("message").and_then(Value::as_str).unwrap_or("no reason given")));
        }
        let expected = auth::proof(&token, Role::Runner, &runner_nonce, &client_nonce, "primary", port);
        let theirs = answer.pointer("/result/proof").and_then(Value::as_str).unwrap_or("");
        if !auth::proof_matches(&expected, theirs) {
            return Err("Whatever is on the runner's port couldn't prove it's the runner, so this app won't talk to it".into());
        }

        // Signed in: a writer fed by `call`, and this task reading.
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        *self.out.lock().unwrap() = Some(tx);
        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
            let _ = write.close().await;
        });

        let me = self.clone();
        let url_owned = url.to_string();
        let hello = tokio::spawn(async move {
            let mut params = json!({ "client": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") });
            // The Browser pane and BrowserBear's handoff (docs/protocol.md,
            // "Showing panels"): BrowserBear is a Mac thing.
            if cfg!(target_os = "macos") {
                params["browserPanel"] = json!(true);
            }
            if let Some(build) = HELLO_BUILD.get() {
                params["build"] = build.clone();
            }
            match me.call("hello", params).await {
                Ok(hello) => {
                    // Whatever restart there was is over; a new pending one
                    // comes back with hello's notice and runner.status.
                    me.update_restart(|r| r.connected());
                    *me.last_down.lock().unwrap() = None;
                    log::write("connection", &format!("connected to runner {} at {url_owned}", hello["runnerVersion"].as_str().unwrap_or("?")));
                    let session = me.sessions.fetch_add(1, Ordering::SeqCst) + 1;
                    me.set_status(Status::Connected { url: url_owned, hello });
                    me.session.send_replace(session);
                }
                Err(e) => log::write("connection", &format!("hello failed: {}", e.message)),
            }
        });

        while let Some(msg) = read.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            let v = match serde_json::from_str::<Value>(text.as_str()) {
                Ok(v) => v,
                Err(e) => {
                    // Say so: a reply dropped here leaves its call waiting
                    // out its whole timeout with nothing in the log (14 Nov:
                    // half an emoji in one event, a "\ud83e" serde refuses,
                    // kept a thread from opening). Where it broke, never
                    // what it says, as it may hold anything.
                    log::write("rpc", &format!("dropped a message from the runner that isn't JSON ({} bytes): {e}", text.len()));
                    continue;
                }
            };
            self.dispatch(v);
        }
        hello.abort();
        self.out.lock().unwrap().take();
        let _ = writer.await;
        Ok(())
    }

    fn dispatch(&self, v: Value) {
        let id = v.get("id").and_then(Value::as_u64);
        let is_reply = v.get("result").is_some() || v.get("error").is_some();
        if let (Some(id), true) = (id, is_reply) {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                let result = match v.get("error") {
                    Some(e) => Err(RpcError {
                        code: e.get("code").and_then(Value::as_i64).unwrap_or(-32000),
                        message: e.get("message").and_then(Value::as_str).unwrap_or("The runner gave no reason").to_string(),
                        data: e.get("data").cloned(),
                    }),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(result);
            }
        } else if let Some(method) = v.get("method").and_then(Value::as_str) {
            let params = v.get("params").unwrap_or(&Value::Null);
            if method == "runner.notice" {
                let (level, message) = (params["level"].as_str().unwrap_or("info"), params["message"].as_str().unwrap_or(""));
                log::write("notice", &format!("{level}: {message}"));
                self.update_restart(|r| {
                    r.notice(message, Instant::now());
                });
            }
            self.sink.notification(method, params);
        }
    }
}

async fn next_json<S>(read: &mut S) -> Result<Value, String>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).map_err(|e| format!("The runner sent something that isn't JSON: {e}")),
            // The runner closes with 4401 when the proof doesn't match: the
            // token isn't its (Transport.swift reads the same code).
            Ok(Message::Close(Some(frame))) if u16::from(frame.code) == 4401 => return Err(format!("{TURNED_DOWN}: the token in secrets/runner-token isn't the runner's")),
            Ok(Message::Close(_)) => return Err("The runner closed the connection while signing in".into()),
            Ok(_) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("The runner closed the connection while signing in".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RunnerAPI`'s longer waits: a chip's command, a slow parse.
    #[test]
    fn slow_requests_get_longer_to_answer() {
        assert_eq!(timeout_for("actions.run", &json!({ "actionId": "worktree.land" })), Duration::from_secs(61 * 60));
        assert_eq!(timeout_for("actions.run", &json!({ "actionId": "checks.quick" })), Duration::from_secs(11 * 60));
        assert_eq!(timeout_for("threads.hibernateWhen", &json!({})), Duration::from_secs(150));
        assert_eq!(timeout_for("panels.convexKey", &json!({})), Duration::from_secs(150));
        assert_eq!(timeout_for("threads.list", &json!({})), Duration::from_secs(120));
    }

    #[test]
    fn reasons_are_said_plainly() {
        assert_eq!(plain_reason("Couldn't reach the runner at ws://127.0.0.1:8797/rpc: IO error: Connection refused (os error 61)"), "Nothing is answering on the runner's port, so it isn't running.");
        assert_eq!(plain_reason("The runner closed the connection"), "The runner closed the connection");
        assert!(!plain_reason("Couldn't reach the runner at ws://127.0.0.1:8797/rpc: IO error: unexpected EOF").contains("ws://"));
    }

    #[test]
    fn what_a_restart_turned_away() {
        let e = |code, message: &str| RpcError { code, message: message.into(), data: None };
        assert!(e(THREAD_NOT_FOUND, "Thread not found").turned_away_by_restart());
        assert!(e(BUSY, "The runner is handing over to a new one; send again in a moment.").turned_away_by_restart());
        assert!(e(BUSY, "The runner is restarting; send again in a moment.").turned_away_by_restart());
        assert!(!e(BUSY, "The thread is busy").turned_away_by_restart());
        assert!(e(NOT_CONNECTED, "Not connected to the runner").turned_away_by_restart());
        // It may have arrived: that's for `send_through_restart` to find out.
        assert!(!e(CLOSED, "The connection to the runner closed").turned_away_by_restart());
    }
}
