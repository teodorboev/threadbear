// One view per section of the pane beside the thread (Highlights, Files,
// Patches, a patch's panels), as the Swift app's DockPane is one view per
// section. They all draw from the one SidePane, which keeps what's fetched,
// the previews and the folders, but each draws its own section wherever the
// dock puts it: Highlights beside Files shows both, not one of them twice.
use crate::highlights::SidePane;
use gpui_kit::*;

pub(crate) struct SectionView {
    side: Entity<SidePane>,
    section: String,
    /// The thread it last drew for, to fetch what a section needs when it
    /// first shows for a thread (a folder's listing, a panel's rows).
    drew_for: Option<String>,
    _watch: Subscription,
}

impl SectionView {
    pub(crate) fn new(side: Entity<SidePane>, section: String, cx: &mut Context<Self>) -> Self {
        // The dock keeps a section's last frame until it's told otherwise, and
        // what it draws lives in the pane.
        let watch = cx.observe(&side, |_, _, cx| cx.notify());
        SectionView { side, section, drew_for: None, _watch: watch }
    }
}

impl Render for SectionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thread = self.side.read(cx).thread(cx);
        if let Some(thread) = &thread {
            let first = self.drew_for.as_deref() != Some(thread.id.as_str());
            // A list or picture panel asks for fresh rows while it's on show;
            // shown again after being closed, it starts asking again.
            let panel = thread.panels().iter().any(|p| p.id == self.section && (p.kind == "list" || p.kind == "image"));
            let restart = panel && !self.side.read(cx).panel_polling(&thread.id, &self.section);
            if first || restart {
                self.drew_for = Some(thread.id.clone());
                // Afterwards: it fetches, which notifies the pane mid-draw otherwise.
                let (side, section, id) = (self.side.clone(), self.section.clone(), thread.id.clone());
                cx.defer(move |cx| side.update(cx, |side, cx| side.section_shown(&section, &id, cx)));
            }
        }
        let section = self.section.clone();
        self.side.update(cx, |side, cx| side.render_section(&section, window, cx))
    }
}
