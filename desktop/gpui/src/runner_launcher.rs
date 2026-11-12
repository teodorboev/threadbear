// The connection banner's Start runner: starts the installed runner's
// LaunchAgent with launchctl, as the Swift app's RunnerLauncher does. Only
// for an app on the installed runner's own data folder
// (`Threadbear::can_start_runner`), and only when Mike presses it: nothing
// here runs by itself.

/// The LaunchAgent install-runner.sh makes (RunnerConfig.launchdLabel).
#[cfg(target_os = "macos")]
const LABEL: &str = "com.mikerosoft.threadbear.runner";

/// Starts it, loading it first from its plist if it isn't loaded. A plain
/// sentence for the error bar when that didn't work, None when launchctl
/// took it. Blocks while launchctl runs, so it's called off the main thread.
#[cfg(target_os = "macos")]
pub(crate) fn start() -> Option<String> {
    let Some(uid) = uid() else { return Some("Couldn't tell which user to start the runner for.".into()) };
    let domain = format!("gui/{uid}");
    let service = format!("{domain}/{LABEL}");
    if launchctl(&["kickstart", &service]) {
        return None;
    }
    // Not loaded yet: load it from its plist, then start it.
    let plist = dirs_home().join("Library/LaunchAgents").join(format!("{LABEL}.plist"));
    if !plist.exists() {
        return Some("The runner isn't installed. Run runner/scripts/install-runner.sh in the threadbear repo.".into());
    }
    launchctl(&["bootstrap", &domain, &plist.to_string_lossy()]);
    if launchctl(&["kickstart", &service]) {
        return None;
    }
    Some(format!("launchctl couldn't start {LABEL}."))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn start() -> Option<String> {
    Some("Start the runner from its own service on this computer.".into())
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> bool {
    std::process::Command::new("/bin/launchctl").args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(target_os = "macos")]
fn uid() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/id").arg("-u").output().ok()?;
    let uid = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit())).then_some(uid)
}

#[cfg(target_os = "macos")]
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default()
}
