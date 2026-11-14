// A side panel pinned to its thread (Pin, on the tab's right-click menu).
//
// A patch's panel (a page, the Convex dashboard, a list or a picture)
// ordinarily follows the selected thread: it shows that thread's copy, and
// steps aside for a thread without one. Pinning swaps it, where it sits, for
// a copy that keeps showing the thread it was pinned from, whichever thread
// is picked after, so Mike can watch one project's dashboard while working in
// another's thread (13 Nov). The ordinary panel still opens from the rail and
// follows the selection beside it. Unpin swaps it back.
//
// The pinned copy's id carries the thread and the panel
// (threadbear_core::pinned_panels), so the saved layout brings it back after
// a relaunch. It has nothing to show once its thread is gone or no longer
// has the panel, so it steps aside then (`panel_available`).
//
// A page is drawn through web_panels.rs as the ordinary panel's is, with a
// page of its own (`pinned_key`); a list or a picture through the side
// pane's own drawing, in a view of its own (`PinnedPanelView`), as the side
// pane draws one section at a time for the selected thread.
use crate::app::Threadbear;
use crate::highlights::SidePane;
use crate::style::{self, px};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use threadbear_core::inbox::ThreadSummary;
pub(crate) use threadbear_core::pinned_panels::{panel as pinned_panel, pinned};

/// A pinned list or picture panel: the side pane's drawing of the thread's
/// panel, redrawn when the pane's data or the app changes.
pub(crate) struct PinnedPanelView {
    app: WeakEntity<Threadbear>,
    side: Entity<SidePane>,
    thread: String,
    panel: String,
    _subs: Vec<Subscription>,
}

impl PinnedPanelView {
    fn new(app: &Entity<Threadbear>, side: Entity<SidePane>, thread: String, panel: String, cx: &mut Context<Self>) -> Self {
        let subs = vec![cx.observe(&side, |_, _, cx| cx.notify()), cx.observe(app, |_, _, cx| cx.notify())];
        PinnedPanelView { app: app.downgrade(), side, thread, panel, _subs: subs }
    }
}

impl Render for PinnedPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thread = self.app.upgrade().and_then(|a| a.read(cx).threads.get(&self.thread).cloned());
        let body = match thread {
            Some(thread) => {
                let (panel, side) = (self.panel.clone(), self.side.clone());
                side.update(cx, |s, cx| {
                    s.keep_panel_fresh(&thread.id, &panel, cx);
                    s.render_panel(&thread, &panel, window, cx)
                })
            }
            None => crate::files::pane_note("This thread has gone.", crate::highlights::palette(cx)).into_any_element(),
        };
        div().size_full().bg(cx.theme().background).child(body)
    }
}

impl Threadbear {
    /// The thread and panel a pinned panel shows, while the thread has it.
    pub(crate) fn pinned_owner(&self, panel: &str) -> Option<(ThreadSummary, threadbear_core::highlights::Panel)> {
        let (thread, id) = pinned(panel)?;
        let thread = self.threads.get(thread)?;
        let found = thread.panels().into_iter().find(|p| p.id == id)?;
        Some(((**thread).clone(), found))
    }

    /// Whether Pin is offered for this panel: one of the selected thread's
    /// patch panels (not on a project's page, where the panels are borrowed
    /// from whichever of its threads has them).
    pub(crate) fn can_pin(&self, panel: &str) -> bool {
        self.projects.open.is_none() && pinned(panel).is_none() && self.pane_thread().is_some_and(|t| t.panels().iter().any(|p| p.id == panel))
    }

    /// The item a panel's right-click menu gets: "Unpin" on a pinned panel,
    /// "Pin" on one of the selected thread's patch panels, else none.
    pub(crate) fn pin_menu_label(&self, panel: &str) -> Option<&'static str> {
        if pinned(panel).is_some() {
            Some("Unpin")
        } else if self.can_pin(panel) {
            Some("Pin")
        } else {
            None
        }
    }

    /// The menu item, chosen.
    pub(crate) fn choose_pin_menu(&mut self, panel: &str, cx: &mut Context<Self>) {
        match self.pin_menu_label(panel) {
            Some("Unpin") => self.unpin_panel(panel, cx),
            Some(_) => self.pin_panel(panel, cx),
            None => {}
        }
    }

    /// The `pinnedPanels` part of the dump: each pinned panel in the layout,
    /// whose it is, whether it's showing, and a list's rows.
    pub(crate) fn pinned_panels_dump(&self, cx: &App) -> serde_json::Value {
        let dock = self.dock.read(cx);
        let list: Vec<serde_json::Value> = dock
            .layout()
            .panels()
            .iter()
            .filter_map(|id| {
                let (thread, panel) = pinned(id)?;
                let rows: Option<Vec<String>> = self.side.read(cx).panels.renders.get(&format!("{thread}/{panel}")).and_then(|r| r.items.as_ref()).map(|items| items.iter().map(|i| i.title.clone()).collect());
                Some(serde_json::json!({
                    "id": id,
                    "thread": thread,
                    "panel": panel,
                    "title": self.pinned_title(id),
                    "tip": self.pinned_tip(id),
                    "available": self.panel_available(id, cx),
                    "showing": dock.controller.is_showing(id),
                    "rows": rows,
                }))
            })
            .collect();
        serde_json::json!(list)
    }

    /// Pin: the panel becomes its pinned copy for the selected thread, in the
    /// same place. If that thread's copy is pinned already, it's brought to
    /// the front and the ordinary panel closes.
    pub(crate) fn pin_panel(&mut self, panel: &str, cx: &mut Context<Self>) {
        if !self.can_pin(panel) {
            return;
        }
        let Some(thread) = self.pane_thread() else { return };
        let pin = pinned_panel(&thread.id, panel);
        if self.dock.read(cx).layout().contains(&pin) {
            self.dock.update(cx, |m, cx| {
                m.close(panel, cx);
                m.select(&pin, cx);
            });
            return;
        }
        self.dock.update(cx, |m, cx| m.rename(panel, &pin, cx));
    }

    /// Unpin: back to the ordinary panel where the pinned one was, which then
    /// follows the selection again; just closed if the ordinary one's open
    /// already.
    pub(crate) fn unpin_panel(&mut self, pin: &str, cx: &mut Context<Self>) {
        let Some((owner, id)) = pinned(pin) else { return };
        let (owner, id) = (owner.to_owned(), id.to_owned());
        if self.dock.read(cx).layout().contains(&id) {
            self.dock.update(cx, |m, cx| m.close(pin, cx));
        } else {
            self.dock.update(cx, |m, cx| m.rename(pin, &id, cx));
        }
        // The panel brought back is the one in front for the thread it was
        // pinned from, and for the selected thread if that has it too: what
        // it opens (a file, a link) opens over it. The side pane's choice only
        // lives in memory, so after a relaunch, with no thread open yet, the
        // pinned thread opened on Highlights though Unpin had just put
        // Previews back (13 Nov, pinned-panels.feature).
        let mut threads = vec![owner.clone()];
        if let Some(t) = self.pane_thread().filter(|t| t.id != owner && t.panels().iter().any(|p| p.id == id)) {
            threads.push(t.id);
        }
        self.side.update(cx, |side, cx| {
            for thread in &threads {
                side.section_pressed(thread, &id, cx);
            }
        });
    }

    /// The selected thread's pinned copy of a panel, while it's in the
    /// layout: what its rail button brings up instead of a second copy.
    pub(crate) fn own_pin(&self, panel: &str, cx: &App) -> Option<String> {
        let thread = self.pane_thread()?;
        let pin = pinned_panel(&thread.id, panel);
        self.dock.read(cx).layout().contains(&pin).then_some(pin)
    }

    pub(crate) fn pinned_title(&self, panel: &str) -> Option<String> {
        pinned(panel)?;
        Some(match self.pinned_owner(panel) {
            Some((_, found)) => found.title,
            None => pinned(panel).map(|(_, id)| id.to_owned()).unwrap_or_default(),
        })
    }

    /// The tab's tip: whose it is.
    pub(crate) fn pinned_tip(&self, panel: &str) -> Option<String> {
        let (thread, _) = pinned(panel)?;
        let title = self.threads.get(thread).map(|t| if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() })?;
        Some(format!("Pinned to “{title}”"))
    }

    /// What a pinned panel shows: its thread's page, list or picture.
    pub(crate) fn pinned_panel_view(&mut self, panel: &str, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some((thread, found)) = self.pinned_owner(panel) else {
            return crate::files::pane_note("Nothing to show here now.", crate::highlights::palette(cx)).into_any_element();
        };
        if found.kind == "url" {
            // A page can only be in one place: a drag's ghost is an empty panel.
            if ghost {
                return div().size_full().into_any_element();
            }
            if let Some(page) = crate::web_panels::section(self, &thread, &found.id, true, window, cx) {
                return page;
            }
        }
        let app = cx.entity();
        let side = self.side.clone();
        let view = self
            .thread_panels
            .pinned
            .entry(panel.to_owned())
            .or_insert_with(|| cx.new(|cx| PinnedPanelView::new(&app, side, thread.id.clone(), found.id.clone(), cx)))
            .clone();
        AnyView::from(view).into_any_element()
    }

    /// Lets go of the views of pinned panels that have closed.
    pub(crate) fn pinned_panels_layout_changed(&mut self, layout: &threadbear_dock::DockLayout) {
        self.thread_panels.pinned.retain(|id, _| layout.contains(id));
    }
}

/// The tab's icon: a small pin before the panel's own, so the origin badge
/// still sits on the panel's icon's corner.
pub(crate) fn tab_icon(icon: AnyElement, cx: &App) -> AnyElement {
    let color = style::secondary_text(cx);
    h_flex_row().child(crate::icons::symbol("pin").with_size(px(10.)).text_color(color)).child(icon).into_any_element()
}

fn h_flex_row() -> Div {
    div().flex().flex_row().items_center().gap(px(3.))
}
