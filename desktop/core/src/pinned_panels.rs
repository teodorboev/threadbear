// A thread's panel pinned (Pin, on its tab's menu): a copy of one of the side
// panels that keeps showing that thread's panel whichever thread is picked,
// beside the ordinary one, which still follows the selection. Its id carries
// both, so it survives a relaunch in the saved layout like any panel.

/// The ids' start.
pub const PREFIX: &str = "threadbear.pin:";

/// The pinned copy of `panel_id` for `thread_id`. A thread's id has no colon
/// (a UUID), so the first one after the start ends it; a panel's id can have
/// any.
pub fn panel(thread_id: &str, panel_id: &str) -> String {
    format!("{PREFIX}{thread_id}:{panel_id}")
}

/// The thread and panel a pinned panel shows, or None for any other panel.
pub fn pinned(panel: &str) -> Option<(&str, &str)> {
    let rest = panel.strip_prefix(PREFIX)?;
    let (thread, id) = rest.split_once(':')?;
    (!thread.is_empty() && !id.is_empty()).then_some((thread, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_panel_names_its_thread_and_panel() {
        let id = panel("abc-123", "convex");
        assert_eq!(id, "threadbear.pin:abc-123:convex");
        assert_eq!(pinned(&id), Some(("abc-123", "convex")));
        // A panel's id can have colons of its own.
        assert_eq!(pinned(&panel("t", "web.localhost:3000")), Some(("t", "web.localhost:3000")));
        assert_eq!(pinned("threadbear.pin:"), None);
        assert_eq!(pinned("threadbear.pin:t:"), None);
        assert_eq!(pinned("threadbear.pin::convex"), None);
        assert_eq!(pinned("convex"), None);
        assert_eq!(pinned("threadbear.thread:abc"), None);
    }
}
