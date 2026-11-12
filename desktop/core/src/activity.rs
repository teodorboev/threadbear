//! The activity log's lines and files (docs/guide.md, "The activity log"), a
//! port of the Swift app's `ActivityEvent`, `ActivityFiles` and
//! `ActivityKeys`: something done in the app, so an agent working on
//! Threadbear can see how it's really used (`tb activity`). Only what was
//! pressed, opened or chosen, never what's typed or the words of a message or
//! a file. Both apps write the same lines to the same files, so `tb activity
//! --summary` reads them as one; the GPUI app's side is activity.rs there.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One line of the log. Every field but `at` and `kind` is left out when it
/// isn't set, as the Swift app's Codable leaves out nil.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    /// Local time with its offset ("2026-10-29T09:15:02.345+08:00"), so a
    /// line read straight from the file says when it was on Mike's clock.
    pub at: String,
    /// click, menu, key, hover, view, panels, send, approval, window, app,
    /// launch or quit (`ActivityEvent.Kind`).
    pub kind: String,
    /// What was clicked, chosen, pressed or hovered: a control's tooltip, a
    /// menu item ("Thread › Archive") or a key ("⌘N").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The key that chose a menu item ("⌘E").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// "right", "middle" or "button 4"; nothing for a left click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    /// 2 for a double click, and so on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clicks: Option<u32>,
    /// Held down with a click ("⌘", "⌥⇧").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<String>,
    /// For a click no tooltip names, what's under the pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// "main", "settings", "popover" or the window's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// The dock panel it happened in ("threads", a patch panel's id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    /// The thread open at the time, or the one acted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// view: the sidebar's view (inbox, scheduled, projects, librarian).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<String>,
    /// view: a page in place of a thread (projects, librarian, scheduled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// panels: the front panel of each group and any in windows of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub showing: Option<Vec<String>>,
    /// send: a new thread, not yet with an id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<bool>,
    /// send: an edit of an earlier message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<u32>,
    /// send: answers picked on the thread's card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<u32>,
    /// approval: allow, allowAlways, deny or answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// app: active or inactive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// launch: the app's version and the commit it was built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// "desktop" for the GPUI app's lines; the Swift app's have none. Both
    /// write the same files, and with both open their lines are mixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

impl ActivityEvent {
    pub fn new(kind: &str, at: chrono::DateTime<chrono::Local>) -> ActivityEvent {
        ActivityEvent { at: at.to_rfc3339_opts(chrono::SecondsFormat::Millis, false), kind: kind.to_owned(), ..Default::default() }
    }

    /// The event as one line of JSON, without the newline, its keys sorted
    /// as the Swift app's are.
    pub fn line(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or_default();
        let sorted: BTreeMap<String, serde_json::Value> = value.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        serde_json::to_string(&sorted).unwrap_or_default()
    }

    pub fn parse(line: &str) -> Option<ActivityEvent> {
        serde_json::from_str(line).ok()
    }

    /// A tooltip or menu title as a target: its first line (a thread row's
    /// tip has the folder under the title), and not a paragraph.
    pub fn label(text: &str) -> Option<String> {
        let first = text.split('\n').next().unwrap_or("").trim();
        if first.is_empty() {
            return None;
        }
        let limit = 120;
        if first.chars().count() > limit {
            let mut cut: String = first.chars().take(limit - 1).collect();
            cut.push('…');
            return Some(cut);
        }
        Some(first.to_owned())
    }
}

/// How long the files are kept.
pub const KEEP_DAYS: i64 = 30;
/// A day's file stops growing here, so something stuck (a key held down, a
/// loop) can't fill the disk. A busy day is well under 1 MB.
pub const MAX_DAY_BYTES: u64 = 20 * 1024 * 1024;

/// "2026-10-29.jsonl", by the local date.
pub fn file_name(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d.jsonl").to_string()
}

/// The day files from before the last `KEEP_DAYS` days, to delete. Anything
/// not named like a day file is left alone.
pub fn expired(names: &[String], today: chrono::NaiveDate) -> Vec<String> {
    let Some(cutoff) = today.checked_sub_signed(chrono::Duration::days(KEEP_DAYS)) else { return Vec::new() };
    let oldest = file_name(cutoff);
    names
        .iter()
        .filter(|n| {
            let b = n.as_bytes();
            n.len() == 16 && n.ends_with(".jsonl") && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
        })
        // Zero-padded dates sort as text.
        .filter(|n| n.as_str() < oldest.as_str())
        .cloned()
        .collect()
}

/// The keys held, in the order macOS menus show them.
pub fn modifiers(command: bool, control: bool, option: bool, shift: bool) -> String {
    format!("{}{}{}{}", if control { "⌃" } else { "" }, if option { "⌥" } else { "" }, if shift { "⇧" } else { "" }, if command { "⌘" } else { "" })
}

/// Which key presses go in the log, by GPUI's name for the key: only
/// shortcuts, a key with ⌘ or ⌃ held, and Escape. Anything else could be
/// typing, and typing is never recorded, not even which keys. The name is as
/// a menu shows it ("⌘N", "⌥⌘P", "⌘↩").
pub fn shortcut(key: &str, command: bool, control: bool, option: bool, shift: bool) -> Option<String> {
    let name = key_name(key);
    if !command && !control {
        return (key == "escape").then_some(name);
    }
    Some(modifiers(command, control, option, shift) + &name)
}

fn key_name(key: &str) -> String {
    match key {
        "enter" => "↩".into(),
        "tab" => "⇥".into(),
        "space" => "Space".into(),
        "backspace" => "⌫".into(),
        "delete" => "⌦".into(),
        "escape" => "Esc".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "down" => "↓".into(),
        "up" => "↑".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        f if f.len() > 1 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => f.to_uppercase(),
        "" => "key".into(),
        other => other.to_uppercase(),
    }
}

/// A dock panel's id as the log names it: Threadbear's own without their
/// `threadbear.`, a patch panel's as it is.
pub fn panel_name(id: &str) -> String {
    id.strip_prefix("threadbear.").unwrap_or(id).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    /// A line has what was set and nothing else, and reads back the same.
    #[test]
    fn a_line_round_trips() {
        let at = chrono::Local.timestamp_millis_opt(1_793_239_331_500).unwrap();
        let mut event = ActivityEvent::new("click", at);
        event.target = Some("Archive (⌘E)".into());
        event.button = Some("right".into());
        event.thread = Some("2072de5b-8d67-48d5-b814-2c8eb26e5eb4".into());
        let line = event.line();
        assert!(!line.contains('\n'));
        let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&line).unwrap();
        assert_eq!(object.keys().cloned().collect::<std::collections::BTreeSet<_>>(), ["at", "button", "kind", "target", "thread"].map(String::from).into_iter().collect());
        // Sorted, as the Swift app writes them.
        assert!(line.starts_with(r#"{"at":"#) && line.find("\"button\"") < line.find("\"kind\""));
        // Local time, with the offset written the way JavaScript reads it.
        let stamp = object["at"].as_str().unwrap();
        assert_eq!(stamp.len(), "2026-10-29T09:15:31.500+08:00".len(), "{stamp}");
        assert_eq!(&stamp[19..23], ".500");
        assert_eq!(chrono::DateTime::parse_from_rfc3339(stamp).unwrap().timestamp_millis(), 1_793_239_331_500);
        assert_eq!(ActivityEvent::parse(&line), Some(event));
    }

    /// The Swift app's lines read the same.
    #[test]
    fn reads_the_swift_apps_lines() {
        let line = r#"{"at":"2026-11-11T09:01:02.003+08:00","images":2,"kind":"send","thread":"t1"}"#;
        let event = ActivityEvent::parse(line).unwrap();
        assert_eq!((event.kind.as_str(), event.images, event.thread.as_deref()), ("send", Some(2), Some("t1")));
        assert_eq!(event.line(), line);
    }

    #[test]
    fn a_label_is_the_first_line_and_short() {
        assert_eq!(ActivityEvent::label("Fix the sidebar\n~/dev/me/threadbear").as_deref(), Some("Fix the sidebar"));
        assert_eq!(ActivityEvent::label("  \n"), None);
        let long = ActivityEvent::label(&"a".repeat(300)).unwrap();
        assert_eq!(long.chars().count(), 120);
        assert!(long.ends_with('…'));
    }

    /// The last 30 days are kept, and nothing that isn't a day file is touched.
    #[test]
    fn expired_files() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        assert_eq!(file_name(today), "2026-10-29.jsonl");
        let names = ["2026-09-28.jsonl", "2026-09-29.jsonl", "2026-10-29.jsonl", "2025-12-31.jsonl", "notes.txt", "2026-09-01.jsonl.tmp"].map(String::from);
        assert_eq!(expired(&names, today), ["2026-09-28.jsonl", "2025-12-31.jsonl"]);
    }

    /// Shortcuts are recorded by name; typing never is, not even with ⌥ or ⇧
    /// held, and Escape is the one bare key that is.
    #[test]
    fn only_shortcuts_are_recorded() {
        assert_eq!(shortcut("n", true, false, false, false).as_deref(), Some("⌘N"));
        assert_eq!(shortcut("p", true, false, true, false).as_deref(), Some("⌥⌘P"));
        assert_eq!(shortcut("c", true, false, false, true).as_deref(), Some("⇧⌘C"));
        assert_eq!(shortcut("s", true, true, false, false).as_deref(), Some("⌃⌘S"));
        assert_eq!(shortcut("enter", true, false, false, false).as_deref(), Some("⌘↩"));
        assert_eq!(shortcut("[", true, false, false, false).as_deref(), Some("⌘["));
        assert_eq!(shortcut("escape", false, false, false, false).as_deref(), Some("Esc"));
        assert_eq!(shortcut("f5", true, false, false, false).as_deref(), Some("⌘F5"));
        assert_eq!(shortcut("a", false, false, false, false), None);
        assert_eq!(shortcut("a", false, false, false, true), None);
        assert_eq!(shortcut("e", false, false, true, false), None);
        assert_eq!(shortcut("enter", false, false, false, false), None);
        assert_eq!(shortcut("backspace", false, false, false, false), None);
        assert_eq!(shortcut("down", false, false, false, false), None);
    }

    #[test]
    fn panels_are_named_without_threadbears_prefix() {
        assert_eq!(panel_name("threadbear.threads"), "threads");
        assert_eq!(panel_name("convex.dashboard"), "convex.dashboard");
    }
}
