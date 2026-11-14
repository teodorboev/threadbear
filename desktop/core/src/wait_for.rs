// The scripted app's `waitFor` (desktop/gpui/src/automation.rs): until every
// condition given holds on the window's `dump`, or with `"not": true` until
// they don't, as the Swift app's (Automation+Waiting.swift). Read from the
// dump rather than the app's own state, so what a script waits for is what
// it would then read, and this half is testable without a window.
use serde_json::Value;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WaitFor {
    /// A transcript row whose id starts with this.
    pub row: Option<String>,
    /// A transcript row of this kind.
    pub kind: Option<String>,
    /// The thread's status: `thread` (an id or its start), else the selected one's.
    pub status: Option<String>,
    pub thread: Option<String>,
    /// Words in a transcript row.
    pub text: Option<String>,
    /// Words in the banner; "" for no banner.
    pub banner: Option<String>,
    /// "connected", "connecting", "down", "lost" or "unauthorized".
    pub connection: Option<String>,
    /// The selected thread's transcript has loaded.
    pub loaded: Option<bool>,
    /// Connected, with the first thread list in (what dev-app.sh's start waits for).
    pub ready: Option<bool>,
    pub negate: bool,
    pub timeout_seconds: f64,
}

impl WaitFor {
    pub fn from_command(c: &Value) -> Result<Self, String> {
        let s = |k: &str| c[k].as_str().map(str::to_owned);
        let w = WaitFor {
            row: s("row"),
            kind: s("kind"),
            status: s("status"),
            thread: s("thread"),
            text: s("text"),
            banner: s("banner"),
            connection: s("connection"),
            loaded: c["loaded"].as_bool(),
            ready: c["ready"].as_bool(),
            negate: c["not"].as_bool() == Some(true),
            timeout_seconds: c["timeout"].as_f64().filter(|t| *t > 0.).unwrap_or(10.),
        };
        if [&w.row, &w.kind, &w.status, &w.text, &w.banner, &w.connection].iter().all(|o| o.is_none()) && w.loaded.is_none() && w.ready.is_none() {
            return Err("nothing to wait for (row, kind, status, text, banner, connection, loaded, ready)".to_owned());
        }
        Ok(w)
    }

    pub fn holds(&self, dump: &Value) -> bool {
        self.all_hold(dump) != self.negate
    }

    fn all_hold(&self, dump: &Value) -> bool {
        let rows = dump["transcript"]["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let field = |r: &Value, k: &str| r[k].as_str().unwrap_or("").to_owned();
        if let Some(row) = &self.row {
            if !rows.iter().any(|r| field(r, "id").starts_with(row.as_str())) {
                return false;
            }
        }
        if let Some(kind) = &self.kind {
            if !rows.iter().any(|r| field(r, "kind") == *kind) {
                return false;
            }
        }
        if let Some(text) = &self.text {
            if !rows.iter().any(|r| field(r, "text").contains(text.as_str())) {
                return false;
            }
        }
        if let Some(status) = &self.status {
            let found = match &self.thread {
                Some(start) => threads(dump).into_iter().find(|t| t["id"].as_str().is_some_and(|id| id.starts_with(start.as_str()))),
                None => Some(&dump["selection"]).filter(|s| s.is_object()),
            };
            if found.and_then(|t| t["status"].as_str()) != Some(status.as_str()) {
                return false;
            }
        }
        if let Some(banner) = &self.banner {
            let shown = dump["banner"]["text"].as_str().unwrap_or("");
            if if banner.is_empty() { !shown.is_empty() } else { !shown.contains(banner.as_str()) } {
                return false;
            }
        }
        let connection = dump["connection"].as_str().unwrap_or("");
        if let Some(want) = &self.connection {
            if connection != want {
                return false;
            }
        }
        if let Some(loaded) = self.loaded {
            let t = &dump["transcript"];
            let is = t["state"].as_str() == Some("loaded") && t["threadId"].as_str().is_some() && t["threadId"] == dump["selection"]["id"];
            if is != loaded {
                return false;
            }
        }
        if let Some(ready) = self.ready {
            let is = connection == "connected" && dump["sidebar"]["loading"].as_bool() == Some(false);
            if is != ready {
                return false;
            }
        }
        true
    }

    /// What it was waiting for, for the timeout's error.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(r) = &self.row {
            parts.push(format!("row {r}"));
        }
        if let Some(k) = &self.kind {
            parts.push(format!("a {k} row"));
        }
        if let Some(s) = &self.status {
            parts.push(format!("status {s}{}", self.thread.as_ref().map(|t| format!(" on {t}")).unwrap_or_default()));
        }
        if let Some(t) = &self.text {
            parts.push(format!("text {t:?}"));
        }
        if let Some(b) = &self.banner {
            parts.push(if b.is_empty() { "no banner".to_owned() } else { format!("banner {b:?}") });
        }
        if let Some(c) = &self.connection {
            parts.push(format!("connection {c}"));
        }
        if let Some(l) = self.loaded {
            parts.push(if l { "the thread loaded" } else { "no thread loaded" }.to_owned());
        }
        if let Some(r) = self.ready {
            parts.push(if r { "the thread list" } else { "no thread list" }.to_owned());
        }
        format!("{}{}", if self.negate { "not " } else { "" }, parts.join(" and "))
    }
}

fn threads(dump: &Value) -> Vec<&Value> {
    let mut out: Vec<&Value> = dump["sidebar"]["sections"].as_array().into_iter().flatten().flat_map(|s| s["threads"].as_array().into_iter().flatten()).collect();
    if dump["selection"].is_object() {
        out.push(&dump["selection"]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dump() -> Value {
        json!({
            "connection": "connected",
            "selection": { "id": "demo_upload", "status": "running" },
            "sidebar": { "loading": false, "sections": [{ "name": "Inbox", "threads": [{ "id": "demo_idle", "status": "idle" }] }] },
            "transcript": { "threadId": "demo_upload", "state": "loaded", "rows": [{ "id": "u:1", "kind": "user", "text": "Upload the file" }, { "id": "a:2", "kind": "assistant", "text": "Done" }] },
            "banner": null,
        })
    }

    fn wait(c: Value) -> WaitFor {
        WaitFor::from_command(&c).unwrap()
    }

    #[test]
    fn needs_something_to_wait_for() {
        assert!(WaitFor::from_command(&json!({ "cmd": "waitFor", "timeout": 2 })).is_err());
        assert_eq!(wait(json!({ "ready": true })).timeout_seconds, 10.);
        assert_eq!(wait(json!({ "ready": true, "timeout": 2.5 })).timeout_seconds, 2.5);
    }

    #[test]
    fn rows_kinds_and_words() {
        let d = dump();
        assert!(wait(json!({ "row": "a:" })).holds(&d));
        assert!(!wait(json!({ "row": "g:" })).holds(&d));
        assert!(wait(json!({ "kind": "assistant", "text": "Upload" })).holds(&d));
        assert!(!wait(json!({ "kind": "assistant", "text": "Nope" })).holds(&d));
        assert!(wait(json!({ "text": "Nope", "not": true })).holds(&d));
    }

    #[test]
    fn statuses_by_thread_or_the_selection() {
        let d = dump();
        assert!(wait(json!({ "status": "running" })).holds(&d));
        assert!(wait(json!({ "status": "idle", "thread": "demo_id" })).holds(&d));
        assert!(!wait(json!({ "status": "running", "thread": "demo_id" })).holds(&d));
        assert!(!wait(json!({ "status": "idle", "thread": "nobody" })).holds(&d));
    }

    #[test]
    fn banner_connection_loaded_ready() {
        let mut d = dump();
        assert!(wait(json!({ "banner": "" })).holds(&d));
        assert!(wait(json!({ "connection": "connected", "loaded": true, "ready": true })).holds(&d));
        d["banner"] = json!({ "text": "Connecting to the runner…" });
        d["connection"] = json!("connecting");
        d["sidebar"]["loading"] = json!(true);
        d["transcript"]["state"] = json!("loading");
        assert!(wait(json!({ "banner": "Connecting" })).holds(&d));
        assert!(!wait(json!({ "banner": "" })).holds(&d));
        assert!(wait(json!({ "ready": false, "loaded": false })).holds(&d));
        assert!(!wait(json!({ "ready": true })).holds(&d));
    }

    #[test]
    fn says_what_it_waited_for() {
        assert_eq!(wait(json!({ "row": "a:", "banner": "", "not": true })).describe(), "not row a: and no banner");
    }
}
