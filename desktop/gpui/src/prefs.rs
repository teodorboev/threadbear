// What this app remembers between runs: what the Swift app keeps in
// UserDefaults for these (AppModel: lastAccountId, lastPoolId, lastModels and
// lastEfforts, defaultRunnerId, recentSlashCommands, slashPaletteLayout). One
// small JSON file in the app's data folder, read at launch and written when
// one of them changes; a missing or unreadable file reads as nothing yet.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use threadbear_client::config::RunnerConfig;
use threadbear_core::new_thread::Remembered;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    #[serde(flatten)]
    pub remembered: Remembered,
    /// The commands picked from the slash palette lately, newest first.
    pub recent_slash: Vec<String>,
    /// The size the slash palette and its columns were dragged to
    /// (slash_resize.rs).
    pub slash_layout: threadbear_core::slash::PaletteLayout,
    /// The local pages Mike chose Run them for, a digest each, so a page
    /// rewritten since asks again (`PreviewScriptConsent`).
    pub preview_scripts: threadbear_core::local_preview::ScriptConsent,
    /// The message box's spelling and substitution switches, from its
    /// right-click menu (composer_spelling.rs).
    pub text_checks: crate::text_services::Checks,
}

fn path() -> PathBuf {
    RunnerConfig::from_env().home.join("gpui-prefs.json")
}

pub fn load() -> Prefs {
    std::fs::read(path()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
}

/// Written whole, and only this user's: the recent slash commands say what
/// they've been doing (`private_file`).
pub fn save(prefs: &Prefs) {
    if let Ok(data) = serde_json::to_vec_pretty(prefs) {
        let _ = threadbear_core::private_file::write(&path(), &data);
    }
}
