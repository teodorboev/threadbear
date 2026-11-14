//! Bringing the Swift app's settings and unsent drafts across to this app,
//! once, at its first launch after this came in: Mike has used the Swift app
//! every day, and a new app that has forgotten his zoom, his folded sections
//! and the half-written message in a thread's box isn't one he can move to.
//!
//! The Swift app keeps its settings in UserDefaults (`defaults export
//! com.mikerosoft.threadbear -` gives them as an XML property list, read
//! here) and its drafts in `app-drafts.json` in the data folder. This app's
//! settings file is keyed by the same names (desktop/gpui/src/settings.rs), so
//! each comes across as it is; the window's frame turns from AppKit's
//! bottom-left origin to a top-left one. Only what this app hasn't got of its
//! own is filled in: what Mike has set here already stays.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// A property list's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    String(String),
    Integer(i64),
    Real(f64),
    Bool(bool),
    Data(Vec<u8>),
    Date(String),
    Array(Vec<Plist>),
    Dict(BTreeMap<String, Plist>),
}

impl Plist {
    fn as_str(&self) -> Option<&str> {
        match self {
            Plist::String(s) => Some(s),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Plist::Real(r) => Some(*r),
            Plist::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Plist::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_strings(&self) -> Option<Vec<String>> {
        match self {
            Plist::Array(items) => items.iter().map(|i| i.as_str().map(str::to_owned)).collect(),
            _ => None,
        }
    }
}

/// An XML property list, as `defaults export` writes it, or none if it isn't
/// one. Its DOCTYPE is allowed, and nothing it points to is fetched.
pub fn parse_plist(xml: &str) -> Option<Plist> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(xml, options).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() != "plist" {
        return None;
    }
    root.children().find(roxmltree::Node::is_element).and_then(value)
}

fn value(node: roxmltree::Node<'_, '_>) -> Option<Plist> {
    let text = || node.text().unwrap_or("").to_owned();
    Some(match node.tag_name().name() {
        "string" => Plist::String(text()),
        "integer" => Plist::Integer(text().trim().parse().ok()?),
        "real" => Plist::Real(text().trim().parse().ok()?),
        "true" => Plist::Bool(true),
        "false" => Plist::Bool(false),
        "date" => Plist::Date(text()),
        "data" => {
            use base64::Engine as _;
            let compact: String = text().chars().filter(|c| !c.is_whitespace()).collect();
            Plist::Data(base64::engine::general_purpose::STANDARD.decode(compact).ok()?)
        }
        "array" => Plist::Array(node.children().filter(roxmltree::Node::is_element).filter_map(value).collect()),
        "dict" => {
            let mut dict = BTreeMap::new();
            let mut key: Option<String> = None;
            for child in node.children().filter(roxmltree::Node::is_element) {
                if child.tag_name().name() == "key" {
                    key = Some(child.text().unwrap_or("").to_owned());
                } else if let Some(k) = key.take() {
                    if let Some(v) = value(child) {
                        dict.insert(k, v);
                    }
                }
            }
            Plist::Dict(dict)
        }
        _ => return None,
    })
}

/// "{{-0, 32}, {2560, 918}}" (NSStringFromRect, AppKit's bottom-left origin on
/// the main screen) to `windowFrame`'s top-left x, y, width and height.
pub fn window_frame(rect: &str, primary_screen_height: f64) -> Option<Value> {
    let numbers: Vec<f64> = rect.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|s| !s.is_empty()).map(|s| s.parse::<f64>()).collect::<Result<_, _>>().ok()?;
    let [x, y, width, height] = numbers[..] else { return None };
    (width > 0. && height > 0.).then(|| json!({ "x": x + 0., "y": primary_screen_height - (y + height), "width": width, "height": height }))
}

/// The settings this app keeps (desktop-settings.json), from the Swift
/// app's defaults, under this app's names for them: each the same value
/// under the same key, but for the frame and the panel layout (saved as
/// JSON data there).
pub fn settings(defaults: &BTreeMap<String, Plist>, primary_screen_height: f64) -> Map<String, Value> {
    let mut out = Map::new();
    let mut put = |key: &str, v: Option<Value>| {
        if let Some(v) = v {
            out.insert(key.to_owned(), v);
        }
    };
    let get = |key: &str| defaults.get(key);
    // TextZoom keeps it only when it isn't 100.
    put("textZoomPercent", get("textZoomPercent").and_then(Plist::as_f64).filter(|z| *z != 100.).map(|z| json!(z as i64)));
    for key in ["collapsedSections", "collapsedScheduleSections", "projectsList.openRows"] {
        put(key, get(key).and_then(Plist::as_strings).map(Value::from));
    }
    for key in ["inboxOrder", "completionSoundDing", "runnersPanelOpen", "usage.chartRange"] {
        put(key, get(key).and_then(Plist::as_str).map(Value::from));
    }
    // "system" is what none means here.
    put("appearance", get("appearance").and_then(Plist::as_str).filter(|a| *a != "system").map(Value::from));
    for key in ["completionSound", "askBeforeQuitting", "activityLog", "usage.showModelWeeks"] {
        put(key, get(key).and_then(Plist::as_bool).map(Value::from));
    }
    put("completionSoundVolume", get("completionSoundVolume").and_then(Plist::as_f64).map(Value::from));
    put(
        "dockLayout",
        match get("dockLayout") {
            Some(Plist::Data(bytes)) => serde_json::from_slice::<Value>(bytes).ok().filter(Value::is_object),
            _ => None,
        },
    );
    put("windowFrame", get("Threadbear window frame main").and_then(Plist::as_str).and_then(|r| window_frame(r, primary_screen_height)));
    out
}

/// The small files this app keeps the rest in, made from the Swift app's
/// defaults: each file's name, what goes in it, and the key the note names it
/// by. Only those the Swift app has anything for.
pub fn side_files(defaults: &BTreeMap<String, Plist>) -> Vec<(&'static str, Value, &'static str)> {
    let mut out = Vec::new();
    // Highlights' folded headings (highlights_store.rs): the list as it is.
    if let Some(folds) = defaults.get("collapsedHighlights").and_then(Plist::as_strings) {
        out.push(("desktop-highlights.json", Value::from(folds), "collapsedHighlights"));
    }
    // What a click on a link does (reply_links.rs), the same values.
    let mut links = Map::new();
    for (swift, here) in [("transcriptFileLinkClick", "fileLinkClick"), ("webLinkClick", "webLinkClick")] {
        if let Some(v) = defaults.get(swift).and_then(Plist::as_str) {
            links.insert(here.to_owned(), Value::from(v));
        }
    }
    if !links.is_empty() {
        out.push(("desktop-links.json", Value::Object(links), "linkClicks"));
    }
    // How files and folders show in the pane (files_store.rs).
    let mut files = Map::new();
    for (swift, here) in [("filesMarkdownView", "markdownView"), ("folderBrowserMode", "folderMode"), ("folderBrowserSort", "folderSort")] {
        if let Some(v) = defaults.get(swift).and_then(Plist::as_str) {
            files.insert(here.to_owned(), Value::from(v));
        }
    }
    for (swift, here) in [("folderBrowserSortAscending", "folderSortAscending"), ("folderBrowserShowHidden", "folderShowHidden")] {
        if let Some(v) = defaults.get(swift).and_then(Plist::as_bool) {
            files.insert(here.to_owned(), Value::from(v));
        }
    }
    if !files.is_empty() {
        out.push(("desktop-files.json", Value::Object(files), "filePrefs"));
    }
    out
}

/// The tour's, the pages' intros' and Finish setting up's state, from the
/// Swift app's defaults, under this app's names: whether the tour's been
/// seen (`tourSeen`), whether the list was hidden (`setupHidden`), and the
/// intros seen (`tourIntro.<name>`, one list here). Only what the Swift app
/// has: a Mac that never ran it gives none, and gets the tour.
pub fn tour_state(defaults: &BTreeMap<String, Plist>) -> Map<String, Value> {
    let mut out = Map::new();
    let on = |key: &str| defaults.get(key).and_then(Plist::as_bool).or_else(|| defaults.get(key).and_then(Plist::as_f64).map(|n| n != 0.));
    for key in ["tourSeen", "setupHidden"] {
        if let Some(v) = on(key) {
            out.insert(key.to_owned(), Value::from(v));
        }
    }
    let intros: Vec<Value> = defaults.keys().filter_map(|k| k.strip_prefix("tourIntro.")).filter(|name| on(&format!("tourIntro.{name}")) == Some(true)).map(Value::from).collect();
    if !intros.is_empty() {
        out.insert("tourIntrosSeen".to_owned(), Value::Array(intros));
    }
    out
}

/// The note's names, in the order it says them: the same order whichever
/// order the keys came in (serde_json's maps keep insertion order in the app,
/// where GPUI turns that on, and sort their keys in core's own tests).
const NOTE_ORDER: [&str; 12] = [
    "zoom",
    "appearance",
    "window's place",
    "panel layout",
    "folded sections",
    "Inbox order",
    "ding",
    "ask before quitting",
    "activity log",
    "where links open",
    "how files and folders show",
    "other settings",
];

/// What a setting is called in the note that says what came across.
pub fn setting_name(key: &str) -> &'static str {
    match key {
        "textZoomPercent" => "zoom",
        "collapsedSections" | "collapsedScheduleSections" | "projectsList.openRows" | "collapsedHighlights" => "folded sections",
        "inboxOrder" => "Inbox order",
        "appearance" => "appearance",
        "dockLayout" => "panel layout",
        "windowFrame" => "window's place",
        "completionSound" | "completionSoundDing" | "completionSoundVolume" => "ding",
        "askBeforeQuitting" => "ask before quitting",
        "activityLog" => "activity log",
        "linkClicks" => "where links open",
        "filePrefs" => "how files and folders show",
        _ => "other settings",
    }
}

/// Puts `imported` into `current` where it has nothing of its own, and says
/// which keys it filled.
pub fn fill_missing(current: &mut Map<String, Value>, imported: Map<String, Value>) -> Vec<String> {
    let mut filled = Vec::new();
    for (key, value) in imported {
        if current.get(&key).is_none_or(Value::is_null) {
            current.insert(key.clone(), value);
            filled.push(key);
        }
    }
    filled
}

/// The Swift app's drafts (`app-drafts.json`: each box's text and pictures,
/// by thread, "new" for the new-thread page, the same shape as this app's
/// `desktop-drafts.json`) put into this app's where its box is empty. Says
/// how many came across.
pub fn fill_drafts(current: &mut Map<String, Value>, swift: &Value) -> usize {
    let empty = |d: &Value| d["text"].as_str().is_none_or(|t| t.trim().is_empty()) && d["images"].as_array().is_none_or(Vec::is_empty);
    let mut count = 0;
    for (key, draft) in swift.as_object().into_iter().flatten() {
        if empty(draft) || current.get(key).is_some_and(|d| !empty(d)) {
            continue;
        }
        current.insert(key.clone(), json!({ "text": draft["text"].as_str().unwrap_or(""), "images": draft["images"].as_array().cloned().unwrap_or_default() }));
        count += 1;
    }
    count
}

/// The note that says what came across: "Brought across from the Swift
/// app: your zoom and panel layout, and 2 unsent drafts." None when nothing
/// did.
pub fn note(keys: &[String], drafts: usize) -> Option<String> {
    let mut names: Vec<&str> = keys.iter().map(|k| setting_name(k)).collect();
    names.sort_by_key(|n| NOTE_ORDER.iter().position(|o| o == n));
    names.dedup();
    let settings = match names.split_last() {
        None => None,
        Some((last, [])) => Some(format!("your {last}")),
        Some((last, rest)) => Some(format!("your {} and {last}", rest.join(", "))),
    };
    let drafts = match drafts {
        0 => None,
        1 => Some("an unsent draft".to_owned()),
        n => Some(format!("{n} unsent drafts")),
    };
    let what = match (settings, drafts) {
        (Some(s), Some(d)) => format!("{s}, and {d}"),
        (Some(s), None) => s,
        (None, Some(d)) => d,
        (None, None) => return None,
    };
    Some(format!("Brought across from the Swift app: {what}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cut down from `defaults export com.mikerosoft.threadbear -` on Mike's
    /// Mac, the values made up.
    const EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Threadbear window frame main</key>
	<string>{{-0, 32}, {1400, 800}}</string>
	<key>activityLog</key>
	<true/>
	<key>collapsedSections</key>
	<array>
		<string>hibernating</string>
		<string>archived</string>
	</array>
	<key>dockLayout</key>
	<data>
	eyJmbG9hdGluZyI6W10sInJvb3QiOnsi
	Z3JvdXAiOnt9fX0=
	</data>
	<key>inboxOrder</key>
	<string>newest</string>
	<key>panelWidth</key>
	<real>671.7578125</real>
	<key>textZoomPercent</key>
	<integer>90</integer>
	<key>completionSoundVolume</key>
	<real>0.4</real>
	<key>dismissedRecaps</key>
	<dict>
		<key>t1</key>
		<true/>
	</dict>
</dict>
</plist>
"#;

    fn defaults() -> BTreeMap<String, Plist> {
        match parse_plist(EXPORT) {
            Some(Plist::Dict(d)) => d,
            other => panic!("not a dict: {other:?}"),
        }
    }

    #[test]
    fn the_tour_comes_across_as_it_was_left() {
        let mut d = BTreeMap::new();
        d.insert("tourSeen".to_owned(), Plist::Bool(true));
        d.insert("tourIntro.projects".to_owned(), Plist::Bool(true));
        d.insert("tourIntro.usage".to_owned(), Plist::Integer(1));
        d.insert("tourIntro.pools".to_owned(), Plist::Bool(false));
        let got = tour_state(&d);
        assert_eq!(Value::Object(got), json!({ "tourSeen": true, "tourIntrosSeen": ["projects", "usage"] }));
        // A Mac that never ran the Swift app: nothing, so it gets the tour.
        assert!(tour_state(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn reads_what_defaults_export_writes() {
        let d = defaults();
        assert_eq!(d["textZoomPercent"], Plist::Integer(90));
        assert_eq!(d["activityLog"], Plist::Bool(true));
        assert_eq!(d["dockLayout"], Plist::Data(br#"{"floating":[],"root":{"group":{}}}"#.to_vec()));
        assert!(matches!(&d["dismissedRecaps"], Plist::Dict(inner) if inner["t1"] == Plist::Bool(true)));
        assert_eq!(parse_plist("not xml"), None);
        assert_eq!(parse_plist("<html></html>"), None);
    }

    #[test]
    fn the_settings_come_across_under_this_apps_names() {
        let s = settings(&defaults(), 950.);
        assert_eq!(s["textZoomPercent"], json!(90));
        assert_eq!(s["collapsedSections"], json!(["hibernating", "archived"]));
        assert_eq!(s["inboxOrder"], json!("newest"));
        assert_eq!(s["activityLog"], json!(true));
        assert_eq!(s["completionSoundVolume"], json!(0.4));
        assert_eq!(s["dockLayout"], json!({ "floating": [], "root": { "group": {} } }));
        // 950 - (32 + 800): AppKit's bottom-left origin to a top-left one.
        assert_eq!(s["windowFrame"], json!({ "x": 0.0, "y": 118.0, "width": 1400.0, "height": 800.0 }));
        // Only what this app keeps.
        assert!(!s.contains_key("panelWidth") && !s.contains_key("dismissedRecaps"));
    }

    #[test]
    fn the_rest_go_in_this_apps_own_small_files() {
        let mut d = defaults();
        d.insert("collapsedHighlights".into(), Plist::Array(vec![Plist::String("Files".into())]));
        d.insert("transcriptFileLinkClick".into(), Plist::String("panel".into()));
        d.insert("folderBrowserMode".into(), Plist::String("icons".into()));
        d.insert("folderBrowserShowHidden".into(), Plist::Bool(true));
        let files = side_files(&d);
        assert_eq!(files[0], ("desktop-highlights.json", json!(["Files"]), "collapsedHighlights"));
        assert_eq!(files[1], ("desktop-links.json", json!({ "fileLinkClick": "panel" }), "linkClicks"));
        assert_eq!(files[2], ("desktop-files.json", json!({ "folderMode": "icons", "folderShowHidden": true }), "filePrefs"));
        assert!(side_files(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn a_frame_that_isnt_one_comes_across_as_none() {
        assert_eq!(window_frame("{{10, 20}, {0, 300}}", 900.), None);
        assert_eq!(window_frame("nonsense", 900.), None);
    }

    #[test]
    fn what_this_app_has_already_stays() {
        let mut current = json!({ "textZoomPercent": 125, "inboxOrder": null }).as_object().cloned().unwrap();
        let filled = fill_missing(&mut current, settings(&defaults(), 950.));
        assert_eq!(current["textZoomPercent"], json!(125));
        assert_eq!(current["inboxOrder"], json!("newest"));
        assert!(filled.contains(&"inboxOrder".to_owned()) && !filled.contains(&"textZoomPercent".to_owned()));
    }

    #[test]
    fn drafts_fill_only_empty_boxes() {
        let swift = json!({
            "t1": { "text": "half a thought \u{FFFC}", "images": [{ "path": "/tmp/a.png", "mediaType": "image/png" }] },
            "t2": { "text": "this one's here already", "images": [] },
            "t3": { "text": "  ", "images": [] },
            "new": { "text": "a new thread", "images": [] },
        });
        let mut current = json!({ "t2": { "text": "mine", "images": [] } }).as_object().cloned().unwrap();
        assert_eq!(fill_drafts(&mut current, &swift), 2);
        assert_eq!(current["t1"]["images"][0]["path"], "/tmp/a.png");
        assert_eq!(current["t2"]["text"], "mine");
        assert!(!current.contains_key("t3"));
        assert_eq!(current["new"]["text"], "a new thread");
    }

    #[test]
    fn the_note_says_what_came_across_in_plain_words() {
        let keys: Vec<String> = ["textZoomPercent", "collapsedSections", "collapsedScheduleSections", "usage.chartRange", "dockLayout"].map(String::from).to_vec();
        assert_eq!(note(&keys, 2).as_deref(), Some("Brought across from the Swift app: your zoom, panel layout, folded sections and other settings, and 2 unsent drafts."));
        // The same words whichever order the keys come in.
        let mut reversed = keys.clone();
        reversed.reverse();
        assert_eq!(note(&reversed, 2), note(&keys, 2));
        assert_eq!(note(&["inboxOrder".to_owned(), "textZoomPercent".to_owned()], 1).as_deref(), Some("Brought across from the Swift app: your zoom and Inbox order, and an unsent draft."));
        assert_eq!(note(&["dockLayout".to_owned()], 0).as_deref(), Some("Brought across from the Swift app: your panel layout."));
        assert_eq!(note(&[], 1).as_deref(), Some("Brought across from the Swift app: an unsent draft."));
        assert_eq!(note(&[], 0), None);
    }
}
