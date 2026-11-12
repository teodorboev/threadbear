// The runner's settings as the Settings window shows them (docs/protocol.md,
// "Settings and models"), all kept in the runner's runner.json, and the words
// around them: a port of the Swift app's RunnerSettings and SettingsText.
// Decoded as forgivingly as the other models, so a missing field takes its
// default and an older runner's fields are ignored.
use serde_json::Value;

/// How often a fork's runner starts a thread to bring in Mike's releases:
/// every so many days, only for ones he marks important, or never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BringIn {
    Every(i64),
    Important,
    Off,
}

impl BringIn {
    pub const STANDARD: BringIn = BringIn::Every(5);

    pub fn json(self) -> Value {
        match self {
            BringIn::Every(days) => Value::from(days),
            BringIn::Important => Value::from("important"),
            BringIn::Off => Value::from("off"),
        }
    }

    pub fn title(self) -> String {
        match self {
            BringIn::Every(1) => "Every day".into(),
            BringIn::Every(7) => "Every week".into(),
            BringIn::Every(14) => "Every 2 weeks".into(),
            BringIn::Every(n) => format!("Every {n} days"),
            BringIn::Important => "Only important ones".into(),
            BringIn::Off => "Never".into(),
        }
    }

    /// The choices, with a custom number of days from runner.json kept as its
    /// own one (`bringInChoices`).
    pub fn choices(current: BringIn) -> Vec<BringIn> {
        let mut days = vec![1, 3, 5, 7, 14];
        if let BringIn::Every(n) = current {
            if !days.contains(&n) {
                days.push(n);
                days.sort();
            }
        }
        days.into_iter().map(BringIn::Every).chain([BringIn::Important, BringIn::Off]).collect()
    }
}

/// One experiment (runner/src/experiments/<id>.md) and its switch.
#[derive(Clone, Debug, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    /// The guide's paragraph, as Markdown.
    pub does: String,
    pub on: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunnerSettings {
    pub default_cwd: String,
    /// Review by another lab: on unless turned off.
    pub lab_review: bool,
    /// Whether the runner sends `labReview` at all; an older one doesn't, and
    /// Settings then shows no switch.
    pub lab_review_supported: bool,
    /// Keep the Mac out of idle sleep while threads work. None from a runner
    /// that isn't on a Mac (or is too old), and Settings then hides it.
    pub keep_awake: Option<bool>,
    pub bring_in: BringIn,
    pub bring_in_supported: bool,
    /// The pool whose Claude logins answer small jobs (titles, the
    /// librarian); None means none, so those are off.
    pub pool: Option<String>,
    /// Whether the runner sends `models.pool` at all.
    pub pool_supported: bool,
    /// Folders whose thread text is never sent, as written ("~/" kept).
    pub private_folders: Vec<String>,
    /// Every experiment there is, oldest first.
    pub experiments: Vec<Experiment>,
}

impl RunnerSettings {
    pub fn decode(v: &Value) -> RunnerSettings {
        let bring_in = match &v["bringIn"] {
            Value::Number(n) => n.as_i64().filter(|d| *d >= 1).map(BringIn::Every),
            Value::String(s) if s == "important" => Some(BringIn::Important),
            Value::String(s) if s == "off" => Some(BringIn::Off),
            _ => None,
        };
        let models = &v["models"];
        RunnerSettings {
            default_cwd: v["defaultCwd"].as_str().unwrap_or("").to_owned(),
            lab_review: v["labReview"].as_bool().unwrap_or(true),
            lab_review_supported: v.get("labReview").is_some_and(Value::is_boolean),
            keep_awake: v["keepAwake"].as_bool(),
            bring_in_supported: bring_in.is_some(),
            bring_in: bring_in.unwrap_or(BringIn::STANDARD),
            pool: models["pool"].as_str().filter(|p| !p.is_empty()).map(str::to_owned),
            pool_supported: models.get("pool").is_some(),
            private_folders: models["privateFolders"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
            experiments: v["experiments"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let id = e["id"].as_str()?.to_owned();
                    Some(Experiment { name: e["name"].as_str().map_or_else(|| id.clone(), str::to_owned), does: e["does"].as_str().unwrap_or("").to_owned(), on: e["on"].as_bool().unwrap_or(false), id })
                })
                .collect(),
        }
    }
}

/// An experiment's paragraph as plain text for its ⓘ: the guide writes it in
/// Markdown, so code marks and doubled spaces go (`experimentAbout`).
pub fn experiment_about(does: &str) -> String {
    let unmarked = does.replace('`', "");
    let mut out = String::with_capacity(unmarked.len());
    let mut spaces = 0;
    for c in unmarked.chars() {
        if c == ' ' || c == '\t' {
            spaces += 1;
            continue;
        }
        if spaces > 0 {
            out.push(' ');
            spaces = 0;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// Standing instructions (docs/protocol.md, "Standing instructions"): what
/// every thread's agent is given. `outside` is set when the file isn't what
/// Settings last saved, so no agent is given it until Mike decides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StandingInstructions {
    pub text: String,
    pub outside: bool,
}

impl StandingInstructions {
    pub fn decode(v: &Value) -> StandingInstructions {
        StandingInstructions { text: v["text"].as_str().unwrap_or("").to_owned(), outside: v["outside"].as_bool().unwrap_or(false) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_read_leniently() {
        let s = RunnerSettings::decode(&json!({
            "defaultCwd": "/Users/m/dev/me/misc", "labReview": false, "keepAwake": true, "bringIn": "important",
            "models": { "pool": "claude", "privateFolders": ["~/private"] },
            "experiments": [{ "id": "gpuiApp", "name": "GPUI app", "does": "The `GPUI`  app.", "on": true }, { "id": "bare" }],
        }));
        assert!(!s.lab_review && s.lab_review_supported);
        assert_eq!(s.keep_awake, Some(true));
        assert_eq!(s.bring_in, BringIn::Important);
        assert_eq!(s.pool.as_deref(), Some("claude"));
        assert_eq!(s.private_folders, vec!["~/private"]);
        assert_eq!(s.experiments[1].name, "bare", "an experiment with no name goes by its id");
        assert_eq!(experiment_about(&s.experiments[0].does), "The GPUI app.");
        let old = RunnerSettings::decode(&json!({ "defaultCwd": "/x", "models": {} }));
        assert!(old.lab_review && !old.lab_review_supported, "an older runner shows no Code review switch");
        assert!(!old.pool_supported && old.keep_awake.is_none() && !old.bring_in_supported);
        let none = RunnerSettings::decode(&json!({ "models": { "pool": null } }));
        assert!(none.pool_supported && none.pool.is_none(), "null is None, and the runner has the setting");
    }

    #[test]
    fn bring_in_keeps_a_custom_number_of_days() {
        let titles: Vec<String> = BringIn::choices(BringIn::Every(10)).into_iter().map(BringIn::title).collect();
        assert_eq!(titles, vec!["Every day", "Every 3 days", "Every 5 days", "Every week", "Every 10 days", "Every 2 weeks", "Only important ones", "Never"]);
    }
}
