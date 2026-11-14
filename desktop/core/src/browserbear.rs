//! BrowserBear (github.com/mikecann/browserbear), the agents' browser, as the
//! app sees it: the Browser pane's pages and the sign-in handoff
//! (docs/protocol.md, "The Browser panel" and "Showing panels"). A port of the
//! Swift app's `AgentBrowserPanel.swift`, `BrowserBear.swift` and
//! `BrowserBearPipe.swift`; the socket itself is the GPUI app's
//! (`agent_browser.rs`), this is what's said over it and what it means.
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The dock's id for the pane section, as the Swift app's `AgentBrowser.panelId`.
pub const PANEL_ID: &str = "threadbear.browser";

/// BrowserBear's id for the agents' own browser, as opposed to one of Mike's
/// Chrome profiles.
pub const AGENTS_BROWSER: &str = "agent-chrome";

/// BrowserBear's folder: `BROWSERBEAR_HOME`, else its usual place. A scripted
/// app (the specs, an agent's dev-app) gets none without `BROWSERBEAR_HOME`,
/// so it never reaches the real BrowserBear on the Mac running it: a click
/// there would bring Mike's real browser windows forward.
pub fn home(env_home: Option<&str>, user_home: &Path, scripted: bool) -> Option<PathBuf> {
    match env_home.filter(|h| !h.is_empty()) {
        Some(h) => Some(PathBuf::from(h)),
        None if scripted => None,
        None => Some(user_home.join("Library/Application Support/BrowserBear")),
    }
}

pub fn socket(home: &Path) -> PathBuf {
    home.join("browserbear.sock")
}

/// What BrowserBear's agent host writes about the agents' browser.
pub fn tabs_file(home: &Path) -> PathBuf {
    home.join("agent/tabs.json")
}

/// A page a thread's agent is using (`AgentTab`). Every field but the tab's
/// number and page is optional, as hosts add fields over time.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentTab {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub tab_id: i64,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub target_id: Option<String>,
    /// In one of Mike's own Chrome profiles rather than the agents' browser.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub bridge: Option<bool>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub profile: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub profile_name: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub profile_email: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub thread: Option<String>,
    /// working, yours or idle (BrowserBear's service/marks.mjs); none when unmarked.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub action: Option<String>,
    /// The agent is waiting on `handoff --wait`, so Done means something.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub finish: Option<bool>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub status: Option<String>,
    /// Goes up each time the agent hands this tab to Mike.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub handoffs: Option<i64>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub handoff_at: Option<f64>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub popup: Option<bool>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub opened_at: Option<f64>,
}

impl AgentTab {
    /// The profile it's in: the same tab number can be in two.
    pub fn profile_id(&self) -> &str {
        self.profile.as_deref().unwrap_or(AGENTS_BROWSER)
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.profile_id(), self.tab_id)
    }

    /// Handed to Mike: amber, first in the list, and Done.
    pub fn yours(&self) -> bool {
        self.state.as_deref() == Some("yours")
    }

    pub fn working(&self) -> bool {
        self.state.as_deref() == Some("working")
    }

    /// "Personal · me@example.com".
    pub fn profile_title(&self) -> String {
        [&self.profile_name, &self.profile_email].into_iter().flatten().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · ")
    }

    /// The page's host, or the whole address when it hasn't one.
    pub fn host(&self) -> String {
        host(&self.url).unwrap_or_else(|| self.url.clone())
    }

    /// Its title, or the host for a page without one yet.
    pub fn shown_title(&self) -> String {
        if self.title.is_empty() { self.host() } else { self.title.clone() }
    }

    /// Where its window is: a Chrome profile's name, else the agents' browser.
    pub fn place(&self, browser_name: &str) -> String {
        if self.bridge == Some(true) {
            let title = self.profile_title();
            if title.is_empty() { "Chrome".to_owned() } else { title }
        } else {
            browser_name.to_owned()
        }
    }
}

/// `scheme://host[:port]/...`'s host, lower-cased by nobody: as written.
fn host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if authority.starts_with('[') { authority.split_inclusive(']').next()? } else { authority.split(':').next()? };
    (!host.is_empty()).then(|| host.to_owned())
}

/// The agent host's tabs.json.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentState {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub port: i64,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub socket: String,
    /// The browser it runs: Helium, or Google Chrome without it.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub browser: Option<String>,
    #[serde(deserialize_with = "capped_tabs")]
    pub tabs: Vec<AgentTab>,
}

/// The first `MAX_TABS` pages, the rest passed over unbuilt: a few MB of
/// `{}`s would otherwise be a million pages made and thrown away.
fn capped_tabs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<AgentTab>, D::Error> {
    struct Capped;
    impl<'de> serde::de::Visitor<'de> for Capped {
        type Value = Vec<AgentTab>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of pages")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut tabs = vec![];
            for _ in 0..MAX_TABS {
                match seq.next_element::<Value>()? {
                    Some(value) => { if let Some(tab) = crate::wire::read(value) { tabs.push(tab); } }
                    None => return Ok(tabs),
                }
            }
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(tabs)
        }
        // A wrongly typed tabs field leaves the host's other fields usable.
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?.is_some() {}
            crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![])
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> { crate::wire::dropped::<AgentState>("tabs field"); Ok(vec![]) }
    }
    d.deserialize_any(Capped)
}

/// The biggest tabs.json read: a few hundred pages is a few hundred KB.
pub const MAX_TABS_FILE: u64 = 4 * 1024 * 1024;
/// The most pages kept from either place.
pub const MAX_TABS: usize = 2000;

pub fn agent_state(bytes: &[u8]) -> Option<AgentState> {
    serde_json::from_slice(bytes).ok()
}

/// `panelState`'s answer: the connected Chrome profiles and their pages.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PanelState {
    #[serde(deserialize_with = "capped_tabs")]
    pub tabs: Vec<AgentTab>,
}

/// The agents' browser's pages and the Chrome profiles' (`mergeTabs`): one
/// each, a profile's winning a clash, in tab order.
pub fn merge(agent: &[AgentTab], profiles: &[AgentTab]) -> Vec<AgentTab> {
    let mut all: std::collections::HashMap<String, AgentTab> = std::collections::HashMap::new();
    for tab in agent.iter().take(MAX_TABS).chain(profiles.iter().filter(|t| t.bridge == Some(true)).take(MAX_TABS)) {
        all.insert(tab.id(), tab.clone());
    }
    let mut all: Vec<AgentTab> = all.into_values().collect();
    all.sort_by(|a, b| a.tab_id.cmp(&b.tab_id).then_with(|| a.profile_id().cmp(b.profile_id())));
    all
}

/// A thread's pages as the pane lists them: waiting for Mike first, then the newest.
pub fn for_thread(tabs: &[AgentTab], thread: &str) -> Vec<AgentTab> {
    let mut mine: Vec<AgentTab> = tabs.iter().filter(|t| t.thread.as_deref() == Some(thread)).cloned().collect();
    let newest = |t: &AgentTab| t.opened_at.unwrap_or(t.tab_id as f64);
    mine.sort_by(|a, b| b.yours().cmp(&a.yours()).then(newest(b).total_cmp(&newest(a))));
    mine
}

/// What changes when an agent hands one of the thread's pages to Mike (again):
/// the pane opens by itself when this changes and has a page that's his.
/// None while nothing is his.
pub fn handoff_key(tabs: &[AgentTab], thread: &str) -> Option<String> {
    let yours: Vec<String> = tabs.iter().filter(|t| t.thread.as_deref() == Some(thread) && t.yours()).map(|t| format!("{}:{}", t.id(), t.handoffs.unwrap_or(0))).collect();
    (!yours.is_empty()).then(|| format!("{thread}|{}", yours.join(",")))
}

/// What's typed in the pane's address bar as a page to open: an address as
/// it is, a bare name with a dot as https, anything else a search.
pub fn address(typed: &str) -> Option<String> {
    let text = typed.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("://") {
        return Some(text.to_owned());
    }
    if text.contains('.') && !text.contains(' ') {
        return Some(format!("https://{text}"));
    }
    let query: String = text.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "+".to_owned(),
        other => format!("%{other:02X}"),
    }).collect();
    Some(format!("https://www.google.com/search?q={query}"))
}

/// Whether a handoff's reply says the tab came to Mike: its own id and an
/// explicit `ok: true`. Anything else opens the link in the browser as usual.
pub fn handed_off(reply: &[u8], id: u64) -> bool {
    let Ok(message) = serde_json::from_slice::<Value>(reply) else { return false };
    message["id"].as_u64() == Some(id) && message["ok"].as_bool() == Some(true)
}

/// Whether a link could be one an agent has open: only web pages are asked about.
pub fn handoff_candidate(url: &str) -> bool {
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    scheme == "http" || scheme == "https"
}

/// One request line: `{id, cmd, args[, thread]}` and a newline.
pub fn request_line(id: u64, cmd: &str, args: Value, thread: Option<&str>) -> Vec<u8> {
    let mut message = json!({ "id": id, "cmd": cmd, "args": args });
    if let Some(thread) = thread {
        message["thread"] = json!(thread);
    }
    let mut line = serde_json::to_vec(&message).unwrap_or_default();
    line.push(b'\n');
    line
}

/// One line back.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// `{id, ok, result|error}`: the result, or what went wrong.
    Reply { id: u64, answer: Result<Value, String> },
    /// Anything without an id.
    Event(Value),
}

/// None for a line that isn't JSON, which ends the connection, as Swift's does.
pub fn read_line(line: &[u8]) -> Option<Line> {
    let message: Value = serde_json::from_slice(line).ok()?;
    if !message.is_object() {
        return None;
    }
    let Some(id) = message["id"].as_u64() else { return Some(Line::Event(message)) };
    let answer = if message["ok"].as_bool() == Some(false) {
        Err(message["error"].as_str().unwrap_or("BrowserBear couldn't open this page.").to_owned())
    } else {
        Ok(match &message["result"] {
            Value::Object(_) => message["result"].clone(),
            _ => json!({}),
        })
    };
    Some(Line::Reply { id, answer })
}

/// The biggest line kept waiting for its end: past it the other side isn't
/// BrowserBear, or is broken.
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// Whole lines out of reads that split them anywhere (a still is hundreds of
/// KB and comes in many reads; several short lines can share one).
#[derive(Debug, Default)]
pub struct Lines {
    buffer: Vec<u8>,
}

impl Lines {
    /// The lines this read finished, oldest first; Err when one is too long.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let from = self.buffer.len();
        self.buffer.extend_from_slice(bytes);
        let mut lines = vec![];
        let mut start = 0;
        // Only what's new can hold a line's end.
        let mut search = from;
        while let Some(at) = self.buffer[search..].iter().position(|b| *b == b'\n') {
            let end = search + at;
            lines.push(self.buffer[start..end].to_vec());
            start = end + 1;
            search = start;
        }
        self.buffer.drain(..start);
        if self.buffer.len() > MAX_LINE {
            return Err("BrowserBear sent an oversized frame.".to_owned());
        }
        Ok(lines)
    }
}

/// `panelStill`'s answer: a JPEG and when BrowserBear took it (ms since 1970).
pub fn still(result: &Value) -> Option<(Vec<u8>, Option<f64>)> {
    use base64::Engine as _;
    let data = result["data"].as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    Some((bytes, result["at"].as_f64()))
}

/// What the pane says with no pages to list.
pub fn empty_note(error: Option<&str>, available: bool) -> String {
    match error {
        Some(e) => e.to_owned(),
        None if available => "No pages open in this thread.".to_owned(),
        None => "BrowserBear isn't running.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(json: &str) -> AgentTab {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_threads_tabs_decode_from_the_hosts_file() {
        let t = tab(r#"{"tabId":1004,"targetId":"AB","url":"https://example.com/","title":"Example","thread":"t1","state":"yours","action":"over to you","finish":true,"status":"complete","handoffs":2,"openedAt":1,"popup":false}"#);
        assert!(t.tab_id == 1004 && t.thread.as_deref() == Some("t1") && t.yours() && t.handoffs == Some(2));
    }

    #[test]
    fn missing_and_unknown_fields_are_fine() {
        let t = tab(r#"{"tabId":1,"targetId":"A","url":"about:blank","title":"","somethingNew":3}"#);
        assert!(t.thread.is_none() && t.handoffs.is_none());
        let state = agent_state(br#"{"port":9222,"socket":"/x","tabs":[{"tabId":1,"url":"https://a.com/","title":"A"}],"later":true}"#).unwrap();
        assert_eq!((state.port, state.tabs.len(), state.browser), (9222, 1, None));
        assert!(agent_state(b"not json").is_none());
    }

    #[test]
    fn existing_chrome_tabs_keep_their_profile_and_thread() {
        let t = tab(r#"{"tabId":24,"url":"https://github.com/","title":"GitHub","thread":"t1","bridge":true,"profile":"personal-id","profileName":"Personal","profileEmail":"me@example.com"}"#);
        assert_eq!(t.profile_title(), "Personal · me@example.com");
        assert_eq!(t.id(), "personal-id/24");
        assert_eq!(t.place("Helium"), "Personal · me@example.com");
    }

    #[test]
    fn a_tab_in_the_agents_browser_is_kept_apart_from_one_in_chrome() {
        let agent = tab(r#"{"tabId":24,"url":"https://github.com/","title":"GitHub"}"#);
        assert_eq!((agent.profile_id(), agent.id()), (AGENTS_BROWSER, "agent-chrome/24".to_owned()));
        let chrome = tab(r#"{"tabId":24,"url":"https://github.com/","title":"GitHub","bridge":true,"profile":"p"}"#);
        // A profile tab not marked as bridged isn't one of the profiles' pages.
        let stray = tab(r#"{"tabId":25,"url":"https://x.com/","title":"X","profile":"p"}"#);
        let merged = merge(&[agent.clone()], &[chrome.clone(), stray]);
        assert_eq!(merged.iter().map(AgentTab::id).collect::<Vec<_>>(), vec!["agent-chrome/24", "p/24"]);
    }

    #[test]
    fn a_threads_pages_put_its_turn_first_then_the_newest() {
        let tabs = vec![
            tab(r#"{"tabId":1,"url":"https://old.com/","title":"Old","thread":"t1","openedAt":10}"#),
            tab(r#"{"tabId":2,"url":"https://new.com/","title":"New","thread":"t1","openedAt":30}"#),
            tab(r#"{"tabId":3,"url":"https://login.com/","title":"Sign in","thread":"t1","state":"yours","openedAt":5}"#),
            tab(r#"{"tabId":4,"url":"https://other.com/","title":"Other","thread":"t2"}"#),
        ];
        let titles: Vec<String> = for_thread(&tabs, "t1").into_iter().map(|t| t.title).collect();
        assert_eq!(titles, vec!["Sign in", "New", "Old"]);
    }

    #[test]
    fn the_handoff_key_moves_with_each_handoff_and_only_for_mikes_turn() {
        let mut tabs = vec![tab(r#"{"tabId":1,"url":"https://a.com/","title":"A","thread":"t1","state":"working"}"#)];
        assert_eq!(handoff_key(&tabs, "t1"), None);
        tabs[0].state = Some("yours".into());
        tabs[0].handoffs = Some(1);
        let first = handoff_key(&tabs, "t1").unwrap();
        tabs[0].handoffs = Some(2);
        assert_ne!(handoff_key(&tabs, "t1").unwrap(), first, "handed over again");
        assert_eq!(handoff_key(&tabs, "t2"), None, "another thread's");
    }

    #[test]
    fn the_address_bar_takes_addresses_names_and_searches() {
        assert_eq!(address("  "), None);
        assert_eq!(address("https://a.com/x").as_deref(), Some("https://a.com/x"));
        assert_eq!(address("qantas.com").as_deref(), Some("https://qantas.com"));
        assert_eq!(address("cheap flights & hotels").as_deref(), Some("https://www.google.com/search?q=cheap+flights+%26+hotels"));
    }

    #[test]
    fn hosts() {
        assert_eq!(tab(r#"{"tabId":1,"url":"https://user@www.qantas.com:443/login?x=1","title":""}"#).shown_title(), "www.qantas.com");
        assert_eq!(tab(r#"{"tabId":1,"url":"about:blank","title":""}"#).host(), "about:blank");
    }

    #[test]
    fn only_a_plain_yes_to_this_request_counts_as_handed_off() {
        assert!(handed_off(br#"{"id":1,"ok":true,"result":{}}"#, 1));
        assert!(!handed_off(br#"{"id":1}"#, 1), "no ok");
        assert!(!handed_off(br#"{"id":1,"ok":"yes"}"#, 1));
        assert!(!handed_off(br#"{"id":2,"ok":true}"#, 1), "another request's");
        assert!(!handed_off(br#"{"id":1,"ok":false,"error":"no tab"}"#, 1));
        assert!(!handed_off(b"nonsense", 1));
    }

    #[test]
    fn a_huge_list_merges_quickly_and_is_capped() {
        let many: Vec<AgentTab> = (0..100_000).map(|i| AgentTab { tab_id: i, url: format!("https://a{i}.com/"), ..Default::default() }).collect();
        let started = std::time::Instant::now();
        assert_eq!(merge(&many, &many).len(), MAX_TABS);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn a_file_of_a_million_empty_pages_keeps_only_the_cap() {
        let mut text = String::from(r#"{"port":1,"tabs":["#);
        text.push_str(&vec!["{}"; 1_000_000].join(","));
        text.push_str("]}");
        let started = std::time::Instant::now();
        assert_eq!(agent_state(text.as_bytes()).unwrap().tabs.len(), MAX_TABS);
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }

    #[test]
    fn only_web_pages_are_asked_about() {
        assert!(handoff_candidate("https://qantas.com/") && handoff_candidate("HTTP://a.com"));
        assert!(!handoff_candidate("file:///etc/passwd") && !handoff_candidate("mailto:a@b.com"));
    }

    #[test]
    fn requests_and_replies() {
        let line = request_line(7, "panelShow", json!({ "tab": 3, "profile": "agent-chrome" }), Some("t1"));
        assert_eq!(*line.last().unwrap(), b'\n');
        let sent: Value = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
        assert_eq!(sent, json!({ "id": 7, "cmd": "panelShow", "args": { "tab": 3, "profile": "agent-chrome" }, "thread": "t1" }));
        assert_eq!(read_line(br#"{"id":7,"ok":true,"result":{"tabs":[]}}"#), Some(Line::Reply { id: 7, answer: Ok(json!({ "tabs": [] })) }));
        assert_eq!(read_line(br#"{"id":8,"ok":false,"error":"no such tab"}"#), Some(Line::Reply { id: 8, answer: Err("no such tab".into()) }));
        assert_eq!(read_line(br#"{"id":9,"ok":true}"#), Some(Line::Reply { id: 9, answer: Ok(json!({})) }));
        assert!(matches!(read_line(br#"{"method":"frame"}"#), Some(Line::Event(_))));
        assert_eq!(read_line(b"garbage"), None);
    }

    /// Lines straddling reads, several in one read, and a reply after a big
    /// frame in the same read all come out whole and in order.
    #[test]
    fn lines_split_across_and_within_reads() {
        let big = "x".repeat(700_000);
        let text = format!("{{\"a\":\"{big}\"}}\n{{\"b\":1}}\n{{\"c\":\"{big}\"}}\n");
        let bytes = text.as_bytes();
        let mut lines = Lines::default();
        let mut got = vec![];
        let mut i = 0;
        for size in [5, 300_000, 400_036, 7, 650_000].into_iter().chain(std::iter::repeat(100_000)) {
            if i >= bytes.len() {
                break;
            }
            let end = (i + size).min(bytes.len());
            got.extend(lines.push(&bytes[i..end]).unwrap());
            i = end;
        }
        let lens: Vec<usize> = got.iter().map(Vec::len).collect();
        assert_eq!(lens, vec![big.len() + 8, 7, big.len() + 8]);
    }

    #[test]
    fn a_line_with_no_end_past_the_limit_is_refused() {
        let mut lines = Lines::default();
        assert!(lines.push(&vec![b'x'; MAX_LINE + 1]).is_err());
    }

    #[test]
    fn stills_decode() {
        assert_eq!(still(&json!({ "data": "/9j/", "at": 1000.0 })), Some((vec![0xff, 0xd8, 0xff], Some(1000.0))));
        assert_eq!(still(&json!({ "data": null })), None);
    }

    #[test]
    fn a_scripted_app_never_finds_the_real_browserbear() {
        let user = Path::new("/Users/me");
        assert_eq!(home(None, user, true), None);
        assert_eq!(home(None, user, false), Some(PathBuf::from("/Users/me/Library/Application Support/BrowserBear")));
        assert_eq!(home(Some("/tmp/bear"), user, true), Some(PathBuf::from("/tmp/bear")));
    }
}
