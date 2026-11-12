// A thread open in a panel of its own, beside the main one: its panel's id in
// the dock (the Swift app's `WindowDock.threadPanel`, so a saved layout
// carries over from one app to the other).

/// The ids' start. The main panel's id is `threadbear.thread`, the same
/// without its colon, from when it was "the thread" panel.
pub const PREFIX: &str = "threadbear.thread:";

/// The panel for a thread opened on its own.
pub fn panel(thread_id: &str) -> String {
    format!("{PREFIX}{thread_id}")
}

/// The thread a panel of its own shows, or None for any other panel
/// (`WindowDock.pinnedThread`).
pub fn pinned_thread(panel: &str) -> Option<&str> {
    panel.strip_prefix(PREFIX).filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_threads_panel_is_named_for_it() {
        assert_eq!(panel("abc"), "threadbear.thread:abc");
        assert_eq!(pinned_thread("threadbear.thread:abc"), Some("abc"));
        // The main panel's id is the start without its colon.
        assert_eq!(pinned_thread("threadbear.thread"), None);
        assert_eq!(pinned_thread("threadbear.thread:"), None);
        assert_eq!(pinned_thread("threadbear.highlights"), None);
    }
}
