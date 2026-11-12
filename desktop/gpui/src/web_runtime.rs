// The web runtime from the app's side (docs/gpui-port.md, "Choice 1: the web
// view", decided as A: CEF downloaded on first use). One per app: where it's
// got to on this machine (threadbear_core::web_runtime), its download when
// it's asked for, Chromium started in this process the first time a page
// needs it (threadbear_web), and the question each place that needs it asks
// until then, with Not now. Until it's installed, diagrams and pages show
// their source and panels offer the browser, so the app works without it.
use crate::push_button::PushButton;
use crate::style::{self, px};
use futures::StreamExt as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use threadbear_core::web_runtime::{self as core, InstallError, Layout, State};

/// Chromium in this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Engine {
    Off,
    Starting,
    Running,
    Failed(String),
}

pub(crate) struct WebRuntime {
    pub(crate) layout: Option<Layout>,
    pub(crate) state: State,
    /// Not now, for this run of the app: what needs the runtime shows its
    /// source or offers the browser without asking again until a relaunch.
    pub(crate) not_now: bool,
    /// Cancel was pressed and the download hasn't stopped yet.
    pub(crate) cancelling: bool,
    cancel: Arc<AtomicBool>,
    pub(crate) engine: Engine,
}

struct Shared(Entity<WebRuntime>);
impl Global for Shared {}

/// What the download thread tells the app.
enum Update {
    Progress(threadbear_web::Progress),
    Done(Result<(), InstallError>),
}

pub(crate) fn init(cx: &mut App) {
    let layout = Layout::default_root().map(Layout::new);
    let state = match (core::this_archive(), &layout) {
        (Some(archive), Some(layout)) if core::is_installed(layout, &archive.pin()) => State::Ready,
        // A runtime only exists for the Mac so far.
        (Some(_), Some(_)) if cfg!(target_os = "macos") => State::Missing,
        _ => State::Unavailable,
    };
    let entity = cx.new(|_| WebRuntime { layout, state, not_now: false, cancelling: false, cancel: Arc::new(AtomicBool::new(false)), engine: Engine::Off });
    cx.set_global(Shared(entity));
}

pub(crate) fn runtime(cx: &App) -> Option<Entity<WebRuntime>> {
    cx.try_global::<Shared>().map(|s| s.0.clone())
}

/// The helper Chromium's processes run as: beside the app's own binary, in
/// its bundle (desktop/scripts/install-app.sh puts it there) or a cargo
/// target folder.
fn helper_binary() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find the app's own binary: {e}"))?;
    let helper = exe.parent().ok_or("the app's binary has no folder")?.join("threadbear-web-helper");
    if helper.is_file() {
        Ok(helper)
    } else {
        Err("this copy of Threadbear has no web helper beside it (threadbear-web-helper): build it again with desktop/scripts/install-app.sh".into())
    }
}

impl WebRuntime {
    /// Starts the download (and then the unpacking) on a thread of its own.
    /// The state says Downloading at once, so the press shows straight away.
    pub(crate) fn install(&mut self, cx: &mut Context<Self>) {
        self.install_from(None, cx);
    }

    /// `local`: an archive already on disk (only the scripted control passes
    /// one), held to the same pinned hash as a download.
    pub(crate) fn install_from(&mut self, local: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        if self.state.busy() || matches!(self.state, State::Ready | State::Unavailable) {
            return;
        }
        let (Some(archive), Some(layout)) = (core::this_archive(), self.layout.clone()) else { return };
        self.cancel.store(false, Ordering::Release);
        self.cancelling = false;
        self.not_now = false;
        self.state = State::Downloading { received: 0, total: archive.bytes };
        cx.notify();
        let cancel = self.cancel.clone();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Update>();
        let started = std::thread::Builder::new().name("web-runtime-install".into()).spawn(move || {
            let progress = tx.clone();
            let result = threadbear_web::download::install(archive, &layout, local.as_deref(), &mut |p| { let _ = progress.unbounded_send(Update::Progress(p)); }, &cancel);
            let _ = tx.unbounded_send(Update::Done(result));
        });
        if let Err(e) = started {
            self.state = State::Failed(format!("couldn't start the download ({e})"));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            while let Some(update) = rx.next().await {
                let done = matches!(update, Update::Done(_));
                let alive = this.update(cx, |this, cx| {
                    match update {
                        Update::Progress(threadbear_web::Progress::Downloading { received, total }) => this.state = State::Downloading { received, total },
                        Update::Progress(threadbear_web::Progress::Unpacking) => this.state = State::Unpacking,
                        Update::Done(Ok(())) => this.state = State::Ready,
                        Update::Done(Err(InstallError::Cancelled)) => this.state = State::Missing,
                        Update::Done(Err(InstallError::Failed(why))) => this.state = State::Failed(why),
                    }
                    if done {
                        this.cancelling = false;
                    }
                    cx.notify();
                });
                if alive.is_err() || done {
                    break;
                }
            }
        })
        .detach();
    }

    /// Stops a download; the thread notices within a read.
    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.state.busy() {
            self.cancel.store(true, Ordering::Release);
            self.cancelling = true;
            cx.notify();
        }
    }

    pub(crate) fn not_now(&mut self, cx: &mut Context<Self>) {
        self.not_now = true;
        cx.notify();
    }

    /// Whether whatever needs the runtime should ask: not installed, not
    /// turned down this run, and possible here.
    pub(crate) fn asks(&self) -> bool {
        !self.not_now && matches!(self.state, State::Missing | State::Failed(_)) || self.state.busy()
    }

    /// Starts Chromium if it's installed and isn't running yet, and says
    /// whether it's running now. Places that need it call this as they draw;
    /// they're told again (`cx.notify`) when it's up or has failed.
    pub(crate) fn ensure_engine(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.engine {
            Engine::Running => return true,
            Engine::Starting | Engine::Failed(_) => return false,
            Engine::Off => {}
        }
        if self.state != State::Ready {
            return false;
        }
        let Some(layout) = self.layout.clone() else { return false };
        let started = helper_binary().and_then(|helper| {
            let bundle = threadbear_web::bundle_identifier().unwrap_or_else(|| "threadbear.desktop.dev".into());
            let helper = core::make_helpers(&layout, &helper, &bundle)?;
            threadbear_web::start(threadbear_web::Launch { bundle: layout.runtime_bundle(), framework: layout.framework(), helper, profile: layout.profile(), log_file: layout.log_file() })
        });
        match started {
            Ok(()) => {
                self.engine = Engine::Starting;
                cx.spawn(async move |this, cx| {
                    // Chromium's own start takes a second or two; a minute
                    // means something's wrong.
                    let since = std::time::Instant::now();
                    loop {
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                        let engine = match threadbear_web::status() {
                            threadbear_web::Status::Ready => Engine::Running,
                            threadbear_web::Status::Failed(why) => Engine::Failed(why),
                            _ if since.elapsed() > Duration::from_secs(60) => Engine::Failed("Chromium didn't start within a minute".into()),
                            _ => continue,
                        };
                        let _ = this.update(cx, |this, cx| {
                            this.engine = engine;
                            cx.notify();
                        });
                        break;
                    }
                })
                .detach();
            }
            Err(why) => self.engine = Engine::Failed(why),
        }
        cx.notify();
        false
    }

    pub(crate) fn dump(&self) -> Value {
        json!({
            "state": self.state.name(),
            "words": self.state.words(),
            "percent": self.state.percent(),
            "notNow": self.not_now,
            "cancelling": self.cancelling,
            "engine": match &self.engine {
                Engine::Off => "off".to_owned(),
                Engine::Starting => "starting".to_owned(),
                Engine::Running => "running".to_owned(),
                Engine::Failed(why) => format!("failed: {why}"),
            },
        })
    }
}

/// The dump's `webRuntime`.
pub(crate) fn dump(cx: &App) -> Value {
    runtime(cx).map_or(Value::Null, |r| r.read(cx).dump())
}

/// The download's size, as the question says it.
fn megabytes() -> u64 {
    core::this_archive().map_or(135, |a| a.megabytes())
}

/// What a place that needs the runtime shows above its source (or in place
/// of its page): the question with Install and Not now, the download going,
/// or why it failed. None once it's ready, turned down, or impossible here,
/// when the place just shows what it can without it. `wants` finishes "Show
/// …?": "this diagram", "this page", "web pages in this panel". `id` keeps
/// each place's buttons apart.
pub(crate) fn prompt(id: &str, wants: &str, cx: &App) -> Option<AnyElement> {
    let runtime = runtime(cx)?;
    let r = runtime.read(cx);
    if !r.asks() {
        return None;
    }
    let (secondary, label) = (style::secondary_text(cx), style::label_text(cx));
    let words = |text: String| div().text_size(px(style::size::DETAIL)).line_height(px(17.)).text_color(secondary).child(text);
    let row = h_flex().gap(px(8.)).items_center().flex_wrap();
    let id = SharedString::from(id.to_owned());
    let body = match &r.state {
        State::Downloading { received, total } => {
            let done = if *total > 0 { (*received as f32 / *total as f32).clamp(0., 1.) } else { 0. };
            let bar = div().w(px(160.)).h(px(4.)).rounded(px(2.)).bg(style::primary_opacity(cx, 0.12)).child(div().h_full().w(relative(done)).rounded(px(2.)).bg(style::accent(cx)));
            let cancel = runtime.clone();
            row.child(words(format!("Downloading the web runtime, {} of {} MB", received / 1_000_000, (total + 500_000) / 1_000_000)))
                .child(bar)
                .child(PushButton::new(SharedString::from(format!("{id}-cancel")), "Cancel", "Stop downloading the web runtime").busy(r.cancelling).render(cx, move |_, _, cx| cancel.update(cx, |r, cx| r.cancel(cx))))
                .into_any_element()
        }
        State::Unpacking => row.child(Spinner::new().color(secondary).with_size(px(11.))).child(words("Unpacking the web runtime".to_owned())).into_any_element(),
        State::Failed(why) => {
            let (again, later) = (runtime.clone(), runtime.clone());
            v_flex()
                .gap(px(6.))
                .child(div().text_size(px(style::size::DETAIL)).line_height(px(17.)).text_color(style::failed_text(cx)).child(format!("Couldn't install the web runtime: {why}")))
                .child(
                    row.child(PushButton::new(SharedString::from(format!("{id}-retry")), "Try again", "Download the web runtime again").render(cx, move |_, _, cx| again.update(cx, |r, cx| r.install(cx))))
                        .child(PushButton::new(SharedString::from(format!("{id}-not-now")), "Not now", "Show the source, and don't ask again until Threadbear restarts").render(cx, move |_, _, cx| later.update(cx, |r, cx| r.not_now(cx)))),
                )
                .into_any_element()
        }
        _ => {
            let (install, later) = (runtime.clone(), runtime.clone());
            v_flex()
                .gap(px(8.))
                .child(div().text_size(px(style::size::DETAIL)).line_height(px(17.)).text_color(label).child(format!("Show {wants}? It needs the web runtime, a {} MB download, once.", megabytes())))
                .child(
                    row.child(PushButton::new(SharedString::from(format!("{id}-install")), "Install web runtime", "Download Chromium for Threadbear to draw diagrams, pages and web panels").render(cx, move |_, _, cx| install.update(cx, |r, cx| r.install(cx))))
                        .child(PushButton::new(SharedString::from(format!("{id}-not-now")), "Not now", "Show the source, and don't ask again until Threadbear restarts").render(cx, move |_, _, cx| later.update(cx, |r, cx| r.not_now(cx)))),
                )
                .into_any_element()
        }
    };
    Some(body)
}

/// Settings > General's Web runtime row: what it is, where it's got to, and
/// Install or Cancel. (Removing it to free the space is for a later change:
/// it can't go while this run of the app has it loaded.)
pub(crate) fn settings_control(cx: &App) -> AnyElement {
    let Some(runtime) = runtime(cx) else { return div().into_any_element() };
    let r = runtime.read(cx);
    let secondary = style::secondary_text(cx);
    let text = div().id("settings-web-runtime-state").max_w(px(300.)).text_size(px(13.)).text_color(secondary).child(settings_words(cx));
    let mut row = h_flex().gap(px(8.)).items_center().child(text);
    match &r.state {
        State::Missing | State::Failed(_) => {
            let install = runtime.clone();
            row = row.child(PushButton::new("settings-web-runtime-install", "Install", format!("Download the web runtime ({} MB)", megabytes())).render(cx, move |_, _, cx| install.update(cx, |r, cx| r.install(cx))));
        }
        State::Downloading { .. } => {
            let cancel = runtime.clone();
            row = row.child(PushButton::new("settings-web-runtime-cancel", "Cancel", "Stop downloading the web runtime").busy(r.cancelling).render(cx, move |_, _, cx| cancel.update(cx, |r, cx| r.cancel(cx))));
        }
        State::Unpacking => row = row.child(Spinner::new().color(secondary).with_size(px(11.))),
        _ => {}
    }
    row.into_any_element()
}

/// What the Settings row says (and its dump, for the specs).
pub(crate) fn settings_words(cx: &App) -> String {
    let Some(runtime) = runtime(cx) else { return String::new() };
    let r = runtime.read(cx);
    match (&r.state, &r.engine) {
        (State::Ready, Engine::Running) => "Installed, running".to_owned(),
        (State::Ready, Engine::Failed(why)) => format!("Installed, but it didn't start: {why}"),
        (State::Missing, _) if r.not_now => "Not installed (Not now, until Threadbear restarts)".to_owned(),
        (state, _) => state.words(),
    }
}

/// The ⓘ's words for that section.
pub(crate) const SETTINGS_ABOUT: &str = "Diagrams and pages in replies, and web panels, are drawn by Chromium: CEF's official build, which isn't part of the app. It's downloaded the first time something needs it, checked against the hash Threadbear has pinned for it, and kept in Threadbear's data folder (about 330 MB once unpacked). Until then those show their source, or open in your browser.";

// ---------------------------------------------------------------- automation

/// `{"cmd":"webRuntime","do":"install"|"notNow"|"cancel"|"start"|"smoke","wait":<s>}`:
/// what the Settings row and the question's buttons do, then (with `wait`)
/// until it settles. `install` takes `archive`, a copy of the build already
/// on disk, checked as a download is. `smoke` starts Chromium if need be and
/// draws a small page with no window, the check that the runtime really runs
/// here; `save` writes its picture to that path.
#[cfg(feature = "automation")]
pub(crate) async fn automation(cmd: &Value, cx: &mut AsyncApp) -> Result<Value, String> {
    let runtime = cx.update(|cx| runtime(cx)).ok_or("the web runtime isn't set up")?;
    let wait = Duration::from_secs_f64(cmd["wait"].as_f64().unwrap_or(0.));
    let since = std::time::Instant::now();
    match cmd["do"].as_str().unwrap_or("") {
        "install" => {
            let local = cmd["archive"].as_str().filter(|a| !a.is_empty()).map(std::path::PathBuf::from);
            cx.update(|cx| runtime.update(cx, |r, cx| r.install_from(local, cx)));
            while since.elapsed() < wait && cx.update(|cx| runtime.read(cx).state.busy()) {
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
        }
        "notNow" => cx.update(|cx| runtime.update(cx, |r, cx| r.not_now(cx))),
        "cancel" => {
            cx.update(|cx| runtime.update(cx, |r, cx| r.cancel(cx)));
            while since.elapsed() < wait && cx.update(|cx| runtime.read(cx).state.busy()) {
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
        }
        "start" | "smoke" | "probe" => {
            let wait = if wait.is_zero() { Duration::from_secs(60) } else { wait };
            loop {
                let (running, engine) = cx.update(|cx| {
                    let running = runtime.update(cx, |r, cx| r.ensure_engine(cx));
                    (running, runtime.read(cx).engine.clone())
                });
                if running {
                    break;
                }
                if let Engine::Failed(why) = engine {
                    return Err(format!("Chromium didn't start: {why}"));
                }
                if cx.update(|cx| runtime.read(cx).state != State::Ready) {
                    return Err(format!("the web runtime isn't installed ({})", cx.update(|cx| runtime.read(cx).state.words())));
                }
                if since.elapsed() > wait {
                    return Err("Chromium didn't start in time".into());
                }
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
            if cmd["do"] == "smoke" {
                return smoke(cmd["save"].as_str()).await;
            }
            if cmd["do"] == "probe" {
                return probe(cx).await;
            }
        }
        other => return Err(format!("do must be install, notNow, cancel, start, smoke or probe, not {other:?}")),
    }
    Ok(cx.update(|cx| runtime.read(cx).dump()))
}

/// A small sealed page drawn with no window, its picture checked.
#[cfg(all(feature = "automation", target_os = "macos"))]
async fn smoke(save: Option<&str>) -> Result<Value, String> {
    let page = "<!doctype html><title>ok</title><body style=\"margin:0;background:#2a7a5a;color:white;font:24px -apple-system\"><p style=\"margin:20px\">The web runtime draws.</p>";
    let url = format!("data:text/html;charset=utf-8,{}", percent_encode(page));
    let (page, loaded) = threadbear_web::Offscreen::open(&url, threadbear_web::Options { javascript: false, width: 400, height: 120, scale: 2. })?;
    loaded.await.map_err(|_| "the page went away".to_owned())??;
    let png = threadbear_web::picture(&page, 400., 120.).await?;
    if let Some(path) = save {
        std::fs::write(path, &png).map_err(|e| format!("couldn't save the picture: {e}"))?;
    }
    let (width, height) = threadbear_web::png_size(&png).ok_or("what came back isn't a PNG")?;
    Ok(json!({ "ok": true, "width": width, "height": height, "bytes": png.len() }))
}

#[cfg(all(feature = "automation", not(target_os = "macos")))]
async fn smoke(_save: Option<&str>) -> Result<Value, String> {
    Err("there's no web runtime for this system yet".into())
}

/// An agent's html made to try everything a sealed page mustn't, drawn the
/// way the transcript draws one (threadbear_web::draw_page): its scripts
/// would make it 3000 points tall, and everything it names (pictures,
/// styles, fonts, frames, media, prefetches, a refresh, a form) points at a
/// listener on this Mac. It should come out short, and nothing should reach
/// the listener.
#[cfg(all(feature = "automation", target_os = "macos"))]
async fn probe(cx: &mut AsyncApp) -> Result<Value, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let at = format!("http://127.0.0.1:{port}");
    let html = format!(
        r#"<style>@import url({at}/import.css); body{{background:url({at}/bg.png)}} @font-face{{font-family:probe;src:url({at}/font.woff)}} p{{font-family:probe}}</style>
<link rel="stylesheet" href="{at}/link.css"><link rel="dns-prefetch" href="//tb-probe.invalid"><link rel="preconnect" href="{at}">
<link rel="prefetch" href="{at}/prefetch"><link rel="icon" href="{at}/icon.png"><meta http-equiv="refresh" content="0;url={at}/refresh">
<p>probe</p><img src="{at}/img.png"><picture><source srcset="{at}/source.png"><img></picture>
<iframe src="{at}/frame"></iframe><object data="{at}/object"></object><embed src="{at}/embed">
<video src="{at}/video.mp4" autoplay muted></video><audio src="{at}/audio.mp3" autoplay></audio>
<form action="{at}/form" method="post"><input name="q" value="x"><button autofocus>send</button></form>
<svg width="10" height="10"><image href="{at}/svg.png" width="10" height="10"/></svg>
<script>document.body.style.height = '3000px'; fetch('{at}/fetch'); new Image().src = '{at}/script-img'; navigator.sendBeacon('{at}/beacon'); location = '{at}/navigate';</script>
<script src="{at}/script.js"></script><body onload="document.body.style.height = '3000px'">"#
    );
    let drawn = threadbear_web::draw_page(&html, 100, 2.).await?;
    // Anything late (a refresh, a prefetch) has a moment to try.
    cx.background_executor().timer(Duration::from_millis(1500)).await;
    let mut reached = 0;
    while listener.accept().is_ok() {
        reached += 1;
    }
    // And a diagram whose labels, links, click handlers, styles and an init
    // directive try the same from the diagram's own world, where Pele runs
    // (Codex review, 11 Nov). It may not draw at all; nothing should arrive.
    let source = format!(
        r#"%%{{init: {{"securityLevel": "loose", "startOnLoad": true}}}}%%
graph TD
  A["<img src='{at}/label.png' onerror='fetch(`{at}/onerror`)'>"] --> B["<a href='{at}/link'>link</a>"]
  click A "{at}/click"
  click B call fetch("{at}/call")
  style A fill:url({at}/style.png)"#
    );
    let diagram = threadbear_web::draw_diagram(&source, 100, 2.).await;
    cx.background_executor().timer(Duration::from_millis(1500)).await;
    let mut diagram_reached = 0;
    while listener.accept().is_ok() {
        diagram_reached += 1;
    }
    Ok(json!({ "height": drawn.height, "reached": reached, "diagramDrawn": diagram.is_ok(), "diagramReached": diagram_reached }))
}

#[cfg(all(feature = "automation", not(target_os = "macos")))]
async fn probe(_cx: &mut AsyncApp) -> Result<Value, String> {
    Err("there's no web runtime for this system yet".into())
}

/// Everything but letters, digits and a few safe marks, %-escaped, for a
/// data: address.
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3 / 2);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
