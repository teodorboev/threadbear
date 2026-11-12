// Whether this launch is the installed app, the one Mike really runs: a
// release build, running as ~/Applications/Threadbear GPUI.app, not scripted.
// Only that one reports crashes and freezes to Sentry, keeps a hang watch in
// ~/Library/Logs and (later) updates itself, as the Swift app's
// `AppUpdater.isInstalledApp` has it. Agents run debug builds, the specs'
// scripted builds and release builds in temp folders (the perf tools) all
// day, and none of those may report or touch the installed app's files.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use threadbear_core::app_update::{bundle_of, BuildInfo};

pub(crate) struct Launch {
    /// The bundle's build.json, from desktop/scripts/make-bundle.sh.
    pub(crate) build: Option<BuildInfo>,
    pub(crate) installed: bool,
    /// Driven by a script (the specs, an agent): THREADBEAR_AUTOMATION.
    pub(crate) scripted: bool,
}

/// Worked out once, at the first ask.
pub(crate) fn launch() -> &'static Launch {
    static LAUNCH: OnceLock<Launch> = OnceLock::new();
    LAUNCH.get_or_init(|| {
        let bundle = std::env::current_exe().ok().and_then(|exe| bundle_of(&exe));
        let build = bundle.as_deref().and_then(BuildInfo::read);
        let scripted = std::env::var_os("THREADBEAR_AUTOMATION").is_some();
        // A debug build, or one with the scripted control in it, is never the
        // installed app, wherever it is.
        let release = !cfg!(debug_assertions) && !cfg!(feature = "automation");
        let installed = release && !scripted && bundle.as_deref().is_some_and(|b| same(b, &install_dir()));
        Launch { build, installed, scripted }
    })
}

/// Where the installed app lives: ~/Applications/Threadbear GPUI.app, as
/// desktop/scripts/install-app.sh puts it. THREADBEAR_GPUI_APP_DIR names
/// another place, for trying the self-update on a throwaway copy.
pub(crate) fn install_dir() -> PathBuf {
    match std::env::var_os("THREADBEAR_GPUI_APP_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home_dir().join("Applications").join("Threadbear GPUI.app"),
    }
}

/// ~/Library/Logs/Threadbear, where the Swift app's hang watch writes too.
/// The runner keeps the Mac's folder names on every system for now, and so
/// does this.
pub(crate) fn logs_dir() -> PathBuf {
    home_dir().join("Library").join("Logs").join("Threadbear")
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The app talks to the real runner's data folder (not the mock's or a dev
/// runner's), so what it does is Mike's.
pub(crate) fn on_real_data() -> bool {
    let home = threadbear_client::config::RunnerConfig::from_env().home;
    let default = threadbear_client::config::default_home();
    same(&home, &default)
}
