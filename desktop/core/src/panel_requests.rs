//! An agent's `show_panel` (docs/protocol.md, "Showing panels"): which panel
//! a request names, where it opens, and the gate that shows each one once,
//! while Mike is looking at its thread (Swift's `PanelRequests` and
//! `PanelRequestGate`).
use crate::inbox::ThreadSummary;
use serde::Deserialize;
use std::collections::HashSet;

/// A panel the agent asked to show Mike, waiting on the thread for him to
/// look at it (`PanelRequest`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelRequest {
    pub id: String,
    /// "highlights", "patches", "usage", "updates", "connectors", or one of
    /// the thread's panels' ids.
    pub panel: String,
    pub title: String,
    /// "tab", "left", "right", "top", "bottom" or "window".
    #[serde(rename = "where")]
    pub place: Option<String>,
    /// `pin` with `show`: the pin to open (the panel is "highlights").
    pub highlight: Option<String>,
    pub reload: Option<bool>,
    pub at: String,
}

/// What a request shows (`RequestedPanel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestedPanel {
    /// One of the thread's pane sections, by the dock's id: Highlights,
    /// Patches or one of its own panels.
    Section(String),
    Usage,
    Updates,
    Connectors,
}

/// Where it opens if it isn't open (`RequestedPlace`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestedPlace {
    Usual,
    Tab,
    Side(String),
    Window,
}

/// The dock's ids for the pane sections a request can name.
pub const HIGHLIGHTS: &str = "threadbear.highlights";
pub const PATCHES: &str = "threadbear.patches";

/// The panel a request names, or None when the thread hasn't got it (a
/// patch can go between the request and the look), or this app hasn't (the
/// Browser pane).
pub fn panel(request: &PanelRequest, thread: &ThreadSummary) -> Option<RequestedPanel> {
    match request.panel.as_str() {
        "highlights" => Some(RequestedPanel::Section(HIGHLIGHTS.to_owned())),
        "patches" => thread.patches.as_ref().is_some_and(|p| !p.is_empty()).then(|| RequestedPanel::Section(PATCHES.to_owned())),
        "usage" => Some(RequestedPanel::Usage),
        "updates" => Some(RequestedPanel::Updates),
        "connectors" => Some(RequestedPanel::Connectors),
        "threadbear.browser" | "browser" => None,
        other => thread.panels.iter().flatten().any(|p| p["id"].as_str() == Some(other)).then(|| RequestedPanel::Section(other.to_owned())),
    }
}

/// Where it opens if it's closed. Anything unknown is the usual place.
pub fn place(request: &PanelRequest) -> RequestedPlace {
    match request.place.as_deref() {
        Some("tab") => RequestedPlace::Tab,
        Some("window") => RequestedPlace::Window,
        Some(side @ ("left" | "right" | "top" | "bottom")) => RequestedPlace::Side(side.to_owned()),
        _ => RequestedPlace::Usual,
    }
}

/// Which waiting request to show, and when: once each, while Mike is looking
/// at its thread. The runner clears it when the app says it showed it, but
/// the summary that still has it can arrive first, so the ids taken are kept
/// for the rest of the run.
#[derive(Debug, Default)]
pub struct Gate {
    taken: HashSet<String>,
}

impl Gate {
    /// The thread in the main panel's request to show now, if Mike is
    /// looking (the app in front) and it hasn't been shown yet.
    pub fn take(&mut self, thread: Option<&ThreadSummary>, looking: bool) -> Option<PanelRequest> {
        if !looking {
            return None;
        }
        let request = thread?.panel_request.clone()?;
        self.taken.insert(request.id.clone()).then_some(request)
    }

    /// Hands a taken request back, unshown: a pin whose highlights didn't
    /// come, or that Mike moved away from while they were fetched.
    pub fn release(&mut self, id: &str) {
        self.taken.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn thread(over: serde_json::Value) -> ThreadSummary {
        let mut v = json!({ "id": "t1", "title": "T", "status": "idle", "panels": [{ "id": "convex", "title": "Convex" }] });
        v.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_request_names_a_panel_the_thread_has_and_a_place() {
        let request = |panel: &str, place: Option<&str>| PanelRequest { id: "r1".into(), panel: panel.into(), place: place.map(str::to_owned), ..Default::default() };
        let t = thread(json!({}));
        assert_eq!(panel(&request("highlights", None), &t), Some(RequestedPanel::Section(HIGHLIGHTS.into())));
        assert_eq!(panel(&request("convex", None), &t), Some(RequestedPanel::Section("convex".into())));
        assert_eq!(panel(&request("gone", None), &t), None, "a panel the thread hasn't got");
        assert_eq!(panel(&request("patches", None), &t), None, "no patches");
        assert_eq!(panel(&request("usage", None), &t), Some(RequestedPanel::Usage));
        assert_eq!(place(&request("usage", Some("left"))), RequestedPlace::Side("left".into()));
        assert_eq!(place(&request("usage", Some("sideways"))), RequestedPlace::Usual);
        assert_eq!(place(&request("usage", Some("window"))), RequestedPlace::Window);
    }

    #[test]
    fn each_request_is_shown_once_and_only_while_mike_looks() {
        let t = thread(json!({ "panelRequest": { "id": "r1", "panel": "highlights", "title": "Highlights", "at": "2026-11-11T08:00:00Z" } }));
        let mut gate = Gate::default();
        assert!(gate.take(Some(&t), false).is_none(), "not while he's elsewhere");
        assert_eq!(gate.take(Some(&t), true).map(|r| r.id), Some("r1".into()));
        assert!(gate.take(Some(&t), true).is_none(), "the summary that still has it comes again");
        gate.release("r1");
        assert!(gate.take(Some(&t), true).is_some(), "handed back, it's taken again");
    }
}
