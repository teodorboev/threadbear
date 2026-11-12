// threadbear://thread/<id> links from outside the app: BrowserBear's Open
// thread pill on an agent's Chrome tab runs `open` on one, and the thread comes
// up in the window as it does from a notification (the Swift app's
// `application(_:open:)`). The bundle claims the scheme in its Info.plist
// (desktop/scripts/install-app.sh, which also says which of the two apps gets
// the links while both are installed). A link is input from outside the app,
// so all it can do is name a thread to show (threadbear_core::thread_link);
// anything else under the scheme is left alone.
use crate::app::Threadbear;
use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::StreamExt as _;
use gpui_kit::*;
use threadbear_core::thread_link;

/// The links the system hands over, from before the window is made. The
/// system's callback gets no app to work in, so it only queues them, and
/// `listen` takes them from there (macOS can hand one over while the app is
/// still starting, before there's a window to show it in).
pub(crate) fn receive(application: &Application) -> UnboundedReceiver<Vec<String>> {
    let (tx, rx) = unbounded();
    application.on_open_urls(move |urls| {
        let _ = tx.unbounded_send(urls);
    });
    rx
}

/// Shows each link's thread in the main window, in turn.
pub(crate) fn listen(mut links: UnboundedReceiver<Vec<String>>, app: WeakEntity<Threadbear>, window: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(urls) = links.next().await {
            let Some(id) = thread_link::last_thread(urls.iter().map(String::as_str)) else { continue };
            let app = app.clone();
            cx.update(|cx| show(&app, window, id, true, cx));
        }
    })
    .detach();
}

/// The thread a link names, in front: the app and its window come forward
/// (not a script's, which stays behind whatever's on that screen), and the
/// thread opens in the Inbox view, or once the list has loaded
/// (`open_from_outside`).
pub(crate) fn show(app: &WeakEntity<Threadbear>, window: AnyWindowHandle, id: String, activate: bool, cx: &mut App) {
    if activate {
        cx.activate(true);
    }
    window
        .update(cx, |_, window, cx| {
            app.update(cx, |this, cx| this.open_linked_thread(id, activate, window, cx)).ok();
        })
        .ok();
}

impl Threadbear {
    /// A link's thread, once the list is in: one it doesn't have says so
    /// rather than leaving the click with nothing to show for it.
    pub(crate) fn open_linked_thread(&mut self, id: String, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        if activate {
            window.activate_window();
        }
        // Not until the lists it could be in have come: this Mac's, and the
        // other Macs' (which can come later, and a link to one of their
        // threads opened at launch went nowhere: Codex's review, 12 Nov).
        if !self.sidebar.loaded || (!self.threads.contains_key(&id) && !self.remote.heard) {
            self.wait_for_link(id, cx);
            return;
        }
        if !self.threads.contains_key(&id) {
            self.show_error("That link is to a thread that isn't in your sidebar. It was deleted, or it's on a machine this app can't reach.".into(), cx);
            return;
        }
        self.set_mode(crate::shell::SidebarMode::Inbox, cx);
        self.open(id, window, cx);
    }
}

/// How long a link waits for the lists before saying its thread isn't there.
const LINK_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

impl Threadbear {
    /// Holds a link's thread until the lists are in (`try_waiting_link`),
    /// and gives up after a while, saying so.
    fn wait_for_link(&mut self, id: String, cx: &mut Context<Self>) {
        self.waiting_link = Some((id, std::time::Instant::now()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINK_WAIT).await;
            this.update(cx, |this, cx| this.try_waiting_link(cx)).ok();
        })
        .detach();
    }

    /// A list has come: opens the waiting link's thread if it's there now,
    /// says it isn't once both lists are in or it's waited long enough, or
    /// goes on waiting.
    pub(crate) fn try_waiting_link(&mut self, cx: &mut Context<Self>) {
        let Some((id, since)) = self.waiting_link.clone() else { return };
        let there = self.threads.contains_key(&id);
        let lists_in = self.sidebar.loaded && self.remote.heard;
        if !there && !lists_in && since.elapsed() < LINK_WAIT {
            return;
        }
        self.waiting_link = None;
        let (me, window) = (cx.entity().downgrade(), self.main_window);
        // Not from inside this update: opening needs the window, and the
        // window's update comes back to this entity.
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    me.update(cx, |this, cx| {
                        if this.threads.contains_key(&id) && this.sidebar.loaded {
                            this.set_mode(crate::shell::SidebarMode::Inbox, cx);
                            this.open(id, window, cx);
                        } else {
                            this.show_error("That link is to a thread that isn't in your sidebar. It was deleted, or it's on a machine this app can't reach.".into(), cx);
                        }
                    })
                    .ok();
                })
                .ok();
        });
    }
}

/// The automation's `open-url`: a link handed over as macOS hands one to the
/// app, through the same steps.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &serde_json::Value, window: AnyWindowHandle, cx: &mut App) -> Result<serde_json::Value, String> {
    let url = cmd["url"].as_str().ok_or("url is the link")?;
    match thread_link::thread_id(url) {
        Some(id) => {
            // After this command's own update of the window is over.
            let weak = app.downgrade();
            cx.defer(move |cx| show(&weak, window, id, false, cx));
            Ok(serde_json::json!("ok"))
        }
        None => Ok(serde_json::json!("ok not a thread link")),
    }
}
