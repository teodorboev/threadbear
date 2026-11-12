// A button an agent put in its reply (docs/protocol.md, "Reply buttons";
// Swift's ReplyButton.swift): `add_button` stores one on the thread, the
// reply links it as `[title](button:<id>)`, and Mike's press starts a thread
// with the brief or sends the message, once. What it looks like, says and
// tips, and which of two snapshots of it is newer.
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReplyButton {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub start: Option<Start>,
    pub send: Option<Send>,
    pub run: Option<Run>,
    pub running: Option<At>,
    pub pressed: Option<Pressed>,
    pub failed: Option<Failed>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Start {
    pub text: String,
    pub title: Option<String>,
    pub cwd: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Send {
    pub thread_id: String,
    pub thread_title: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Run {
    pub command: String,
    pub cwd: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct At {
    pub at: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Pressed {
    pub at: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Failed {
    pub at: String,
    pub error: String,
}

impl ReplyButton {
    pub fn decode(v: &serde_json::Value) -> Option<ReplyButton> {
        let button: ReplyButton = serde_json::from_value(v.clone()).ok()?;
        (!button.id.is_empty()).then_some(button)
    }

    /// Whether this snapshot is newer than `old`. A button only moves
    /// forward (made, then failed any number of times, then pressed for
    /// good), so its state says which came later even when snapshots arrive
    /// out of order (an older page's event, or `buttons.list` racing a live
    /// `button.updated`).
    pub fn supersedes(&self, old: &ReplyButton) -> bool {
        if old.pressed.is_some() {
            return false;
        }
        if self.pressed.is_some() {
            return true;
        }
        // Running and failing can alternate (a run pressed again), so the
        // later of the two moments wins.
        let when = |b: &ReplyButton| b.running.as_ref().map(|r| r.at.clone()).or_else(|| b.failed.as_ref().map(|f| f.at.clone()));
        match (when(self), when(old)) {
            (None, None) => self != old,
            (None, Some(_)) => false,
            (Some(_), None) => true,
            (Some(new), Some(previous)) => new > previous || (new == previous && self != old),
        }
    }
}

/// How a pill looks (`ReplyButtons.Look`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Start,
    Send,
    Pressing,
    Pressed,
    Failed,
}

pub fn look(button: &ReplyButton, pressing: bool) -> Look {
    if button.pressed.is_some() {
        Look::Pressed
    } else if pressing {
        Look::Pressing
    } else if button.failed.is_some() {
        Look::Failed
    } else if button.send.is_some() {
        Look::Send
    } else {
        Look::Start
    }
}

/// What the pill says: a mark for what it does, then its title.
pub fn label(button: &ReplyButton, look: Look) -> String {
    let mark = match look {
        // U+FE0E keeps ▶ a text glyph rather than the emoji.
        Look::Start => "\u{25B6}\u{FE0E}",
        Look::Send => "\u{21AA}\u{FE0E}",
        Look::Pressing => "\u{2026}",
        Look::Pressed => "\u{2713}",
        Look::Failed => "!",
    };
    format!("{mark} {}", button.title)
}

/// The pill's tip: what pressing does, in the agent's own words.
pub fn tip(button: &ReplyButton, look: Look) -> String {
    let what = if let Some(start) = &button.start {
        let folder = start.cwd.trim_end_matches('/').rsplit('/').next().unwrap_or("");
        format!("Starts a thread in {}: {}", if folder.is_empty() { start.cwd.as_str() } else { folder }, excerpt(&start.text, 200))
    } else if let Some(send) = &button.send {
        format!("Sends to \"{}\": {}", send.thread_title, excerpt(&send.text, 200))
    } else {
        button.title.clone()
    };
    match look {
        Look::Start | Look::Send => what,
        Look::Pressing => format!("On its way. {what}"),
        Look::Pressed => format!("{}\n\n{what}", if button.send.is_some() { "Sent. Click to open the thread it went to." } else { "Started. Click to open the thread it started." }),
        Look::Failed => format!("That didn't work: {}. Click to try again.\n\n{what}", button.failed.as_ref().map_or("unknown error", |f| f.error.as_str())),
    }
}

/// The first `limit` characters, on one line.
pub fn excerpt(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The ids of the buttons a reply links, in order (`ReplyButtons.ids(in:)`).
pub fn ids_in(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](button:") {
        let after = &rest[at + "](button:".len()..];
        let Some(end) = after.find(')') else { break };
        let id = after[..end].trim();
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            ids.push(id.to_owned());
        }
        rest = &after[end..];
    }
    ids
}

impl Look {
    /// Its name in the dumps (`Look.rawValue`).
    pub fn name(self) -> &'static str {
        match self {
            Look::Start => "start",
            Look::Send => "send",
            Look::Pressing => "pressing",
            Look::Pressed => "pressed",
            Look::Failed => "failed",
        }
    }
}

/// The button a `button:` link names.
pub fn id_of(url: &str) -> Option<&str> {
    let (scheme, rest) = url.trim().split_once(':')?;
    if !scheme.eq_ignore_ascii_case("button") {
        return None;
    }
    let id = rest.trim();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn button(v: serde_json::Value) -> ReplyButton {
        ReplyButton::decode(&v).unwrap()
    }

    #[test]
    fn looks_and_says_what_it_does() {
        let start = button(json!({ "id": "abc12345", "title": "Fill in the form", "start": { "text": "Open the form   and\\nfill it in", "cwd": "/Users/m/dev/me/misc" } }));
        assert_eq!(look(&start, false), Look::Start);
        assert_eq!(label(&start, Look::Start), "\u{25B6}\u{FE0E} Fill in the form");
        assert_eq!(tip(&start, Look::Start), "Starts a thread in misc: Open the form and\\nfill it in");
        assert_eq!(look(&start, true), Look::Pressing);
        let send = button(json!({ "id": "x", "title": "Reply yes", "send": { "threadId": "t2", "threadTitle": "Meetup", "text": "yes" } }));
        assert_eq!(label(&send, look(&send, false)), "\u{21AA}\u{FE0E} Reply yes");
        let failed = button(json!({ "id": "x", "title": "Go", "start": { "text": "go", "cwd": "/a" }, "failed": { "at": "2026-11-11T10:00:00Z", "error": "the folder has gone" } }));
        assert!(tip(&failed, look(&failed, false)).starts_with("That didn't work: the folder has gone. Click to try again."));
        let pressed = button(json!({ "id": "x", "title": "Go", "send": { "threadId": "t", "threadTitle": "T", "text": "a" }, "pressed": { "at": "z", "threadId": "t" } }));
        assert_eq!(look(&pressed, true), Look::Pressed, "pressed wins over a press on its way");
        assert!(tip(&pressed, Look::Pressed).starts_with("Sent. Click to open the thread it went to."));
    }

    #[test]
    fn a_newer_snapshot_wins_and_a_pressed_one_stays() {
        let made = button(json!({ "id": "x", "title": "Go" }));
        let failed = button(json!({ "id": "x", "title": "Go", "failed": { "at": "2026-11-11T10:00:00Z", "error": "e" } }));
        let pressed = button(json!({ "id": "x", "title": "Go", "pressed": { "at": "2026-11-11T11:00:00Z", "threadId": "t" } }));
        assert!(failed.supersedes(&made));
        assert!(!made.supersedes(&failed));
        assert!(pressed.supersedes(&failed));
        assert!(!failed.supersedes(&pressed));
        assert!(!made.supersedes(&made));
    }

    #[test]
    fn reads_button_links() {
        assert_eq!(id_of("button:abc12345"), Some("abc12345"));
        assert_eq!(id_of("Button: abc "), Some("abc"));
        assert_eq!(id_of("https://example.com"), None);
        assert_eq!(id_of("button:"), None);
        assert_eq!(excerpt("one two three", 7), "one two\u{2026}");
        assert_eq!(ids_in("a [x](button:ab12) b [y](button: cd34 ) [z](https://e)"), ["ab12", "cd34"]);
    }
}
