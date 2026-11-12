// What the app remembers between launches: where the window was, the zoom,
// which sidebar sections are folded, the Inbox's sort and the panel layout.
// One JSON file in the runner's data folder, beside shortcuts.json, rather
// than the Swift app's UserDefaults: an agent can read it, it needs no
// per-OS code, and there's nothing to migrate between machines.
//
// The field names are the Swift app's UserDefaults keys
// (AppModel.collapsedSectionsKey "collapsedSections",
// AppModel.collapsedScheduleSectionsKey "collapsedScheduleSections", AppModel.inboxOrderKey
// "inboxOrder", TextZoom.defaultsKey "textZoomPercent", WindowDock.layoutKey
// "dockLayout"; MainWindowFrame's "Threadbear window frame main" is
// `windowFrame` here), so a later one-time import from UserDefaults is
// mechanical: read each key across as-is, and for the frame
// parse NSStringFromRect's "{{x, y}, {w, h}}" (bottom-left origin) into a
// top-left one.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use threadbear_client::config::RunnerConfig;
use threadbear_core::completion_sound::{self, Ding};
use threadbear_core::inbox::InboxOrder;
use threadbear_core::zoom;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    /// The window's frame, in points from the top left of the screens.
    pub window_frame: Option<WindowFrame>,
    /// `textZoomPercent`. Only saved when it isn't 100, as TextZoom removes it.
    pub text_zoom_percent: Option<u32>,
    /// `collapsedSections`: the sidebar sections folded away, by
    /// `InboxSection`'s raw value. None means nothing saved yet (Hibernating
    /// and Archived start folded, as `storedCollapsedSections` makes them).
    pub collapsed_sections: Option<Vec<String>>,
    /// `collapsedScheduleSections`: the Scheduled list's headings folded
    /// away, by `ScheduleSection`'s raw value. None means nothing saved yet
    /// (Done starts folded, as `storedCollapsedScheduleSections` makes it).
    pub collapsed_schedule_sections: Option<Vec<String>>,
    /// `inboxOrder`: `InboxOrder`'s raw value.
    pub inbox_order: Option<String>,
    /// `appearance`: "system", "light" or "dark" (`AppAppearance`).
    pub appearance: Option<String>,
    /// `dockLayout`: the Dock's JSON, the same shape Swift's Codable saves.
    pub dock_layout: Option<Value>,
    /// `completionSound`: the ding when a thread finishes (ding.rs). Off
    /// until it's turned on.
    pub completion_sound: Option<bool>,
    /// `completionSoundDing`: which one, `Ding`'s name.
    pub completion_sound_ding: Option<String>,
    /// `completionSoundVolume`: 0.1 to 1.
    pub completion_sound_volume: Option<f64>,
    /// `askBeforeQuitting` (QuitPrompt): None is yes.
    pub ask_before_quitting: Option<bool>,
    /// `activityLog` (ActivityRecorder): Settings > General's Keep an
    /// activity log (activity.rs). Off until it's turned on.
    pub activity_log: Option<bool>,
    /// When the Swift app's settings and drafts were brought across
    /// (swift_import.rs): once, so none is.
    pub swift_import: Option<String>,
    /// The parts of that done so far ("settings", "drafts", "files"), each
    /// once, while another is still to go after a failure.
    pub swift_import_parts: Vec<String>,
    /// `updateRestore`: the sidebar's view and the thread that were open
    /// when the app restarted itself for an update, put back once (app_update.rs).
    pub update_restore: Option<Value>,
    /// `runnersPanelOpen`: the runners whose details are open in the Runners
    /// panel, their ids with commas between.
    pub runners_panel_open: Option<String>,
    /// `usage.chartRange`: the Usage panel's chart, "today" or "week".
    #[serde(rename = "usage.chartRange")]
    pub usage_chart_range: Option<String>,
    /// `usage.showModelWeeks`: the Usage panel's switch for a model's own
    /// week ("Weekly Fable"). None is on.
    #[serde(rename = "usage.showModelWeeks")]
    pub usage_show_model_weeks: Option<bool>,
    /// `projectsList.openRows`: the sidebar's folded project rows that are
    /// open (the projectDecisions experiment), by project id, "-" for Not in
    /// a project.
    #[serde(rename = "projectsList.openRows")]
    pub projects_open_rows: Option<Vec<String>>,
    /// `threadsCollapsed`: ⌘/ folded the Threads panel away (thread_panels.rs).
    /// None is no.
    pub threads_collapsed: Option<bool>,
}

/// The settings file: where it is and what's in it. Reading is at launch only;
/// the rest of the time the app holds the current values and writes the file
/// when one changes.
pub struct Settings {
    path: PathBuf,
    pub saved: Saved,
}

impl Settings {
    pub fn file_name() -> &'static str {
        "desktop-settings.json"
    }

    pub fn path_in(home: &Path) -> PathBuf {
        home.join(Self::file_name())
    }

    /// The file for the runner this app talks to (THREADBEAR_HOME, or its
    /// default): a mock's or a dev runner's settings stay with that runner's
    /// data, never Mike's real one.
    pub fn load() -> Self {
        let path = Self::path_in(&RunnerConfig::from_env().home);
        Self::load_from(path)
    }

    pub fn load_from(path: PathBuf) -> Self {
        let saved = std::fs::read(&path).ok().and_then(|data| serde_json::from_slice::<Saved>(&data).ok()).unwrap_or_default();
        Settings { path, saved }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Written whole and renamed into place, so a crash mid-write can't leave
    /// half a file (the same as shortcuts.json's atomic write in the Swift app),
    /// and only this user's, as everything in the data folder is.
    pub fn save(&self) {
        let _ = self.save_checked();
    }

    /// `save`, saying whether it went, for a record that mustn't be lost
    /// (swift_import.rs).
    pub fn save_checked(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.saved).map_err(|e| e.to_string())?;
        threadbear_core::private_file::write(&self.path, text.as_bytes()).map_err(|e| format!("couldn't save {}: {e}", self.path.display()))
    }

    pub fn zoom_percent(&self) -> u32 {
        zoom::sanitize(self.saved.text_zoom_percent)
    }

    pub fn set_zoom_percent(&mut self, percent: u32) {
        self.saved.text_zoom_percent = (percent != zoom::DEFAULT_PERCENT).then_some(percent);
    }

    pub fn inbox_order(&self) -> InboxOrder {
        self.saved.inbox_order.as_deref().and_then(InboxOrder::from_name).unwrap_or_default()
    }

    pub fn set_inbox_order(&mut self, order: InboxOrder) {
        self.saved.inbox_order = Some(order.name().to_string());
    }

    pub fn ding_on(&self) -> bool {
        self.saved.completion_sound.unwrap_or(false)
    }

    pub fn ding(&self) -> Ding {
        Ding::from_name(self.saved.completion_sound_ding.as_deref().unwrap_or(""))
    }

    pub fn ding_volume(&self) -> f64 {
        self.saved.completion_sound_volume.unwrap_or(completion_sound::DEFAULT_VOLUME).clamp(0.1, 1.)
    }

    pub fn asks_before_quitting(&self) -> bool {
        self.saved.ask_before_quitting.unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-settings-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(Settings::file_name())
    }

    #[test]
    fn what_was_saved_comes_back() {
        let path = temp_file("roundtrip");
        let mut s = Settings::load_from(path.clone());
        assert_eq!(s.saved, Saved::default(), "no file yet");
        s.set_zoom_percent(125);
        s.set_inbox_order(InboxOrder::Newest);
        s.saved.collapsed_sections = Some(vec!["inbox".into()]);
        s.saved.window_frame = Some(WindowFrame { x: 10., y: 20., width: 1180., height: 800. });
        s.save();
        let read = Settings::load_from(path);
        assert_eq!(read.zoom_percent(), 125);
        assert_eq!(read.inbox_order(), InboxOrder::Newest);
        assert_eq!(read.saved.collapsed_sections, Some(vec!["inbox".to_string()]));
        assert_eq!(read.saved.window_frame, Some(WindowFrame { x: 10., y: 20., width: 1180., height: 800. }));
    }

    #[test]
    fn a_broken_file_is_as_good_as_none() {
        let path = temp_file("broken");
        std::fs::write(&path, "{ half").unwrap();
        let s = Settings::load_from(path);
        assert_eq!(s.saved, Saved::default());
    }

    #[test]
    fn unknown_fields_are_kept_out_of_the_way() {
        let path = temp_file("unknown");
        std::fs::write(&path, r#"{"textZoomPercent": 140, "somethingNewer": true}"#).unwrap();
        let s = Settings::load_from(path);
        assert_eq!(s.zoom_percent(), 140);
    }
}
