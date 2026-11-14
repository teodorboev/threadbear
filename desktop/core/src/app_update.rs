//! What the installed app was built from, and the rules it follows main by,
//! a port of the Swift app's `BuildInfo` and `AppUpdate`. The bundle's
//! `Contents/Resources/build.json` says the commit, the tree of `desktop/` at
//! it and the checkout whose main to follow; crash reports go by its origin
//! (only Mike's own repo reports to his Sentry).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `build.json`, as desktop/scripts/make-bundle.sh writes it: the same fields
/// as the Swift app's, so one reader would do for both.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildInfo {
    pub commit: String,
    pub short: String,
    /// The tree hash of `desktop/` at that commit (`git rev-parse
    /// <commit>:desktop`). Only a change under it changes what runs, so this,
    /// not the commit, says whether a build would be any different.
    pub app_tree: String,
    pub built_at: String,
    /// The checkout it was built from, whose main the updater follows.
    pub checkout: String,
    pub configuration: String,
    /// The checkout's origin remote when it was built.
    pub origin: Option<String>,
    /// Where its binaries were built: an ssh host, or "" for this Mac. None
    /// from a make-bundle.sh before 12 Nov 2026, or one not told.
    pub built_on: Option<String>,
}

impl BuildInfo {
    pub fn path_in(bundle: &Path) -> PathBuf {
        bundle.join("Contents").join("Resources").join("build.json")
    }

    pub fn read(bundle: &Path) -> Option<BuildInfo> {
        let data = std::fs::read(Self::path_in(bundle)).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// github.com/mikecann/threadbear, however the URL was written.
    pub fn is_mikes_repo(&self) -> bool {
        self.origin.as_deref().is_some_and(|o| repo_key(o) == "github.com/mikecann/threadbear")
    }
}

/// An origin URL as host/owner/name, lowercased, however it was written
/// (`isMikesRepo`, `repoKey` in runner/src/upstream.ts).
pub fn repo_key(origin: &str) -> String {
    let mut key = origin.trim().to_lowercase();
    if let Some(rest) = key.strip_prefix("git@") {
        key = rest.replace(':', "/");
    }
    if let Some(at) = key.find("://") {
        key = key[at + 3..].to_owned();
    }
    if let Some(at) = key.find('@') {
        if !key[..at].contains('/') {
            key = key[at + 1..].to_owned();
        }
    }
    let key = key.strip_suffix('/').unwrap_or(&key);
    key.strip_suffix(".git").unwrap_or(key).to_owned()
}

/// The bundle an executable runs from: `<bundle>/Contents/MacOS/<exe>`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    if macos.file_name()? != "MacOS" || contents.file_name()? != "Contents" {
        return None;
    }
    contents.parent().map(Path::to_path_buf)
}

/// runner.json's `updateBuildHosts`: the only Macs trusted to build what this
/// one runs (desktop/scripts/stage-update.sh). Check hosts never are.
pub fn update_build_hosts(runner_json: &Path) -> Vec<String> {
    let Ok(data) = std::fs::read(runner_json) else { return Vec::new() };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data) else { return Vec::new() };
    match &json["updateBuildHosts"] {
        serde_json::Value::String(h) => vec![h.clone()],
        serde_json::Value::Array(a) => a.iter().filter_map(|h| h.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

/// Whether a staged build may go in: built on this Mac, or on a host that's
/// in `updateBuildHosts` now. One that doesn't say where it was built was
/// staged before that was asked, when a check host could have built it
/// (Daybreak 108), and one from a host taken off the list since is no longer
/// trusted either.
pub fn builder_trusted(staged: &BuildInfo, update_build_hosts: &[String]) -> bool {
    match staged.built_on.as_deref() {
        Some("") => true,
        Some(host) => update_build_hosts.iter().any(|h| h == host),
        None => false,
    }
}

/// Deletes the staged bundle at `next` if its builder isn't trusted, and says
/// why. Before every swap and every look at what's staged. A swap cut short
/// that left the old bundle at `next` loses it here rather than to `old`,
/// which is the same: it was going. Err when one that isn't trusted is
/// still there (it couldn't be deleted): then nothing may be swapped in.
pub fn drop_untrusted_staged(next: &Path, runner_json: &Path) -> Result<Option<String>, String> {
    let Some(staged) = BuildInfo::read(next) else { return Ok(None) };
    if builder_trusted(&staged, &update_build_hosts(runner_json)) {
        return Ok(None);
    }
    let why = match &staged.built_on {
        Some(host) => format!("the staged build {}: {host} isn't in updateBuildHosts", staged.short),
        None => format!("the staged build {}: it doesn't say which Mac built it", staged.short),
    };
    match std::fs::remove_dir_all(next) {
        _ if !next.exists() => Ok(Some(format!("dropped {why}"))),
        Ok(()) => Err(format!("couldn't drop {why}; not swapping anything in")),
        Err(e) => Err(format!("couldn't drop {why} ({e}); not swapping anything in")),
    }
}

// Following main (the Swift app's `AppUpdate`): what to build, and putting a
// staged build in the installed one's place.

/// `git rev-parse` for main's `desktop/` tree in a checkout: what a build of
/// main would be made from. The build takes three files from the Swift app's
/// folder too (two pictures and the diagram library): a change to only those
/// comes in with the next change under desktop/, rather than every change to
/// the Swift app's resources making a build.
pub fn rev_parse_arguments(checkout: &str) -> Vec<String> {
    ["-C", checkout, "rev-parse", "refs/heads/main:desktop"].map(String::from).to_vec()
}

/// The tree from `git rev-parse`'s output.
pub fn tree_from_rev_parse(output: &str) -> String {
    output.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("+")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Nothing,
    /// Main's tree differs from what's running, no build is staged or under
    /// way, and this tree isn't known to fail.
    Build(String),
    /// What's staged no longer matches main, and main is what's running
    /// already: drop it.
    DropStaged,
}

pub fn decide(main_tree: &str, running: &BuildInfo, staged: Option<&BuildInfo>, building: Option<&str>, failed: Option<&str>, restarting: bool) -> Action {
    if restarting || building.is_some() {
        return Action::Nothing;
    }
    if main_tree == running.app_tree {
        return if staged.is_some() { Action::DropStaged } else { Action::Nothing };
    }
    // Let a staged build go in before starting another. Otherwise an idle
    // restart can cut off the new build just after it starts.
    if staged.is_some() || failed == Some(main_tree) {
        return Action::Nothing;
    }
    Action::Build(main_tree.to_owned())
}

/// Nearby landings become one update build. Compare the desktop tree, so a
/// runner-only landing doesn't extend the wait. Observe during a build too:
/// when it ends, only the newest tree is considered, with its own quiet time.
#[derive(Default)]
pub struct BuildDebounce {
    observed: Option<(String, std::time::Instant)>,
}

impl BuildDebounce {
    pub fn ready(&mut self, tree: &str, now: std::time::Instant) -> bool {
        let (last, since) = self.observed.get_or_insert_with(|| (tree.to_owned(), now));
        if last != tree {
            *last = tree.to_owned();
            *since = now;
        }
        now.saturating_duration_since(*since) >= std::time::Duration::from_secs(10 * 60)
    }
}

/// Puts the staged bundle in the installed one's place in one exchange, so
/// there's never a moment with no app there: two renames left none if the
/// app was killed between them (Codex's review, 12 Nov). The old bundle (the
/// running one, when this is done on quit) then goes to `old`, to be deleted
/// at the next launch. `swapping` notes the incoming build's tree while it
/// happens: killed after the exchange, the old bundle is left at `next`, and
/// the note says it isn't the one to put in (`apply_staged`). Not the build
/// times, which a wrong clock gets wrong (Codex's second look).
pub fn swap(install: &Path, next: &Path, old: &Path, swapping: &Path, incoming: &str) -> std::io::Result<()> {
    if old.exists() {
        std::fs::remove_dir_all(old)?;
    }
    if !install.exists() {
        return std::fs::rename(next, install);
    }
    crate::private_file::write(swapping, incoming.as_bytes())?;
    exchange(next, install)?;
    std::fs::rename(next, old)?;
    std::fs::remove_file(swapping)
}

/// Swaps what two paths name, at once.
#[cfg(target_os = "macos")]
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    let a = std::ffi::CString::new(a.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: two NUL-terminated paths that live until the call returns.
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    let a = std::ffi::CString::new(a.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: two NUL-terminated paths that live until the call returns.
    if unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_EXCHANGE) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Elsewhere (Windows has its own way in), two renames, putting the old one
/// back if the second fails.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    let aside = b.with_extension("swapping");
    std::fs::rename(b, &aside)?;
    if let Err(e) = std::fs::rename(a, b) {
        let _ = std::fs::rename(&aside, b);
        return Err(e);
    }
    std::fs::rename(&aside, a)
}

/// Swaps a staged bundle in if it would change anything, and says what was
/// swapped in. A staged bundle that matches what's running (a build that
/// finished after the same tree went in another way), or that has no
/// build.json, is dropped. Leftovers of an earlier swap go first.
pub fn apply_staged(install: &Path, next: &Path, old: &Path, swapping: &Path, running: Option<&BuildInfo>) -> Result<Option<BuildInfo>, String> {
    if old.exists() {
        let _ = std::fs::remove_dir_all(old);
    }
    // A swap cut short. If what's at `next` isn't the build it was putting
    // in, the exchange happened and that's the old bundle: it goes, rather
    // than back in. If it is, the exchange never happened: on as usual. The
    // note goes only once that's sorted, and a note that can't be read, or a
    // move that fails, stops here, so a later launch still knows (Codex, 12 Nov).
    match std::fs::read_to_string(swapping) {
        Ok(incoming) => {
            let at_next = BuildInfo::read(next).map(|b| b.app_tree);
            if next.exists() && at_next.as_deref() != Some(incoming.trim()) {
                std::fs::rename(next, old).map_err(|e| format!("couldn't move the old bundle left by a swap cut short out of the way: {e}"))?;
            }
            std::fs::remove_file(swapping).map_err(|e| format!("couldn't clear the note of a swap cut short: {e}"))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("couldn't read the note of a swap cut short: {e}")),
    }
    if !next.exists() {
        return Ok(None);
    }
    let Some(staged) = BuildInfo::read(next).filter(|s| running.is_none_or(|r| r.app_tree != s.app_tree)) else {
        let _ = std::fs::remove_dir_all(next);
        return Ok(None);
    };
    swap(install, next, old, swapping, &staged.app_tree).map_err(|e| format!("couldn't put {} in place: {e}", next.display()))?;
    Ok(Some(staged))
}

/// With the app in front, this long with no input at all (a key, a click,
/// the pointer moving, a scroll) counts as not using it. Mike, 12 Nov: "The
/// auto restart should wait for there to be inactivity for five seconds
/// before starting."
pub const AUTO_RESTART_IDLE_SECONDS: f64 = 5.;

/// Whether a field holds words a restart would lose: anything other than
/// what it held when it was made, or last saved, whitespace aside.
pub fn typed_since(saved: &str, now: &str) -> bool {
    saved.trim() != now.trim()
}

/// How long after the last click or key in a web panel's page its restart
/// waits: typing there can't be read from here, so a page used lately
/// counts as being typed in (Codex's review, 12 Nov).
pub const WEB_PAGE_USED_SECONDS: f64 = 60.;

/// Whether a web panel's page holds a restart off: it has the keyboard, or
/// was clicked or typed in within `WEB_PAGE_USED_SECONDS` (`used_ago`, none
/// if never since the app opened).
pub fn web_page_holds(has_keyboard: bool, used_ago: Option<f64>) -> bool {
    has_keyboard || used_ago.is_some_and(|ago| ago < WEB_PAGE_USED_SECONDS)
}

/// What a restart to take a staged build is weighed against, once a second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RestartMoment {
    /// A build is staged.
    pub staged: bool,
    /// The threads are in, so there's a list and a thread to put back.
    pub loaded: bool,
    /// Words or pictures in a box that a restart would lose: a message box,
    /// a schedule being edited, a thread being renamed.
    pub has_drafts: bool,
    /// Something under way that a restart would cut off: a drag (a file, a
    /// row, a selection with the button held), a trackpad gesture, a menu, a
    /// popover, a sheet or a dialog.
    pub held: bool,
    /// A window other than the main one has focus with a field being typed
    /// in (Settings, a review's notes): it would close with the restart.
    pub other_window_input: bool,
    /// The app is in front.
    pub active: bool,
    /// Seconds since the last input.
    pub idle_for: f64,
}

/// Whether to restart on its own to take a staged build: nothing typed would
/// be lost, nothing under way would be cut off, the threads are in, and Mike
/// isn't in the app (another app is in front) or hasn't touched it for
/// `AUTO_RESTART_IDLE_SECONDS`. Running threads don't hold it up: the runner
/// owns the turns and the app replays what it missed when it reconnects
/// (`shouldAutoRestart`).
///
/// The Swift app restarts in front only when its handover would go unseen
/// (the new build comes up under the old window). The GPUI app has no
/// handover, so its window goes and comes back on the thread he had open:
/// Mike would rather that than clicking Restart to Update (12 Nov), as he
/// keeps the app in front and the update otherwise never went in.
pub fn should_auto_restart(m: &RestartMoment) -> bool {
    if !m.staged || !m.loaded || m.has_drafts || m.held || m.other_window_input {
        return false;
    }
    !m.active || m.idle_for >= AUTO_RESTART_IDLE_SECONDS
}

/// What a staged build's restart is waiting on just now, for the Updates
/// panel. Mike, 13 Nov, after one sat staged with no word of why: "can it
/// just have a little countdown number on there or something? And that gets
/// reset when I do something?"
#[derive(Clone, Debug, PartialEq)]
pub enum RestartWait {
    /// Only his input holds it: whole seconds left of the idle wait, back to
    /// the full wait at each key, click or pointer move.
    Countdown(u32),
    /// Something a restart would lose or cut off, as the end of "It
    /// switches in once ..." ("the threads have loaded").
    Held(String),
}

/// What the restart waits on (`should_auto_restart`'s own reasons, in the
/// same order), or none when nothing's staged or it's going now. `why_held`
/// names what's typed or under way, asked only when that's what holds it.
pub fn restart_wait(m: &RestartMoment, why_held: impl FnOnce() -> String) -> Option<RestartWait> {
    if !m.staged {
        return None;
    }
    if !m.loaded {
        return Some(RestartWait::Held("the threads have loaded".to_owned()));
    }
    if m.has_drafts || m.held || m.other_window_input {
        return Some(RestartWait::Held(why_held()));
    }
    if !m.active || m.idle_for >= AUTO_RESTART_IDLE_SECONDS {
        return None;
    }
    // Rounded up, so it reads 5 while he's using the app and 1 last.
    Some(RestartWait::Countdown((AUTO_RESTART_IDLE_SECONDS - m.idle_for.max(0.)).ceil().max(1.) as u32))
}

/// The update script's arguments, run at background priority (`taskpolicy
/// -b`, as Mike asks of every build on his M5): throttled CPU and disk, off
/// the cores he's using, inherited by everything it starts, the copy to a
/// build host and back included.
pub fn build_command(script: &str, checkout: &str, next: &str) -> (String, Vec<String>) {
    ("/usr/sbin/taskpolicy".into(), ["-b", "/bin/bash", script, checkout, "--next", next].map(String::from).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(origin: Option<&str>) -> BuildInfo {
        BuildInfo { origin: origin.map(str::to_owned), ..Default::default() }
    }

    fn build(short: &str, tree: &str) -> BuildInfo {
        BuildInfo { commit: format!("{short}000"), short: short.into(), app_tree: tree.into(), configuration: "release".into(), ..Default::default() }
    }

    /// A bundle folder with a build.json and a marker file naming it.
    fn bundle(dir: &Path, name: &str, info: &BuildInfo) -> PathBuf {
        let url = dir.join(name);
        std::fs::create_dir_all(url.join("Contents/Resources")).unwrap();
        std::fs::write(BuildInfo::path_in(&url), serde_json::to_vec(info).unwrap()).unwrap();
        std::fs::write(url.join("Contents/marker"), &info.short).unwrap();
        url
    }

    #[test]
    fn only_a_build_from_here_or_an_update_build_host_goes_in() {
        let dir = temp("trusted");
        let json = dir.join("runner.json");
        let next = dir.join("next.app");
        let staged = |built_on: Option<&str>| {
            let _ = std::fs::remove_dir_all(&next);
            bundle(&dir, "next.app", &BuildInfo { built_on: built_on.map(str::to_owned), ..build("new", "t2") });
        };
        // Check hosts never count, and a bundle that doesn't say goes.
        std::fs::write(&json, r#"{"checkHosts":["tinker-desk"],"checkHost":"tinker-desk"}"#).unwrap();
        for who in [Some("tinker-desk"), None] {
            staged(who);
            assert!(drop_untrusted_staged(&next, &json).unwrap().is_some(), "{who:?}");
            assert!(!next.exists(), "{who:?}");
        }
        staged(Some(""));
        assert_eq!(drop_untrusted_staged(&next, &json), Ok(None));
        assert!(next.exists());
        std::fs::write(&json, r#"{"checkHosts":["tinker-desk"],"updateBuildHosts":["builder"]}"#).unwrap();
        staged(Some("builder"));
        assert_eq!(drop_untrusted_staged(&next, &json), Ok(None));
        staged(Some("tinker-desk"));
        assert!(drop_untrusted_staged(&next, &json).unwrap().is_some());
        // No runner.json, or one that can't be read: only a build here.
        std::fs::remove_file(&json).unwrap();
        staged(Some("builder"));
        assert!(drop_untrusted_staged(&next, &json).unwrap().is_some());
        // One that can't be deleted is an error, so nothing is swapped in.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            staged(Some("builder"));
            let locked = next.join("Contents");
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
            let held = drop_untrusted_staged(&next, &json);
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(held.is_err(), "{held:?}");
            assert!(next.exists());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn marker(url: &Path) -> Option<String> {
        std::fs::read_to_string(url.join("Contents/marker")).ok()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-app-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_build_runs_at_background_priority() {
        let (program, args) = build_command("/b/stage-update.sh", "/repo", "/next");
        assert_eq!(program, "/usr/sbin/taskpolicy");
        assert_eq!(args, ["-b", "/bin/bash", "/b/stage-update.sh", "/repo", "--next", "/next"]);
    }

    #[test]
    fn decides_whether_to_build() {
        let running = build("aaa", "t1");
        assert_eq!(decide("t1", &running, None, None, None, false), Action::Nothing);
        assert_eq!(decide("t2", &running, None, None, None, false), Action::Build("t2".into()));
        // Already staged, being built or known to fail: leave it.
        assert_eq!(decide("t2", &running, Some(&build("bbb", "t2")), None, None, false), Action::Nothing);
        assert_eq!(decide("t2", &running, None, Some("t2"), None, false), Action::Nothing);
        // A look that finishes after a newer main was read must not start
        // another build alongside the one already running.
        assert_eq!(decide("t3", &running, None, Some("t2"), None, false), Action::Nothing);
        assert_eq!(decide("t2", &running, None, None, Some("t2"), false), Action::Nothing);
        // A staged build goes in first; the new app considers the latest main.
        assert_eq!(decide("t3", &running, Some(&build("bbb", "t2")), None, Some("t2"), false), Action::Nothing);
        assert_eq!(decide("t3", &running, None, None, Some("t2"), false), Action::Build("t3".into()));
        // Main is back to what's running while something else is staged.
        assert_eq!(decide("t1", &running, Some(&build("bbb", "t2")), None, None, false), Action::DropStaged);
        // During handover next has already moved into place, but the old
        // app is still alive. Its pending git read mustn't rebuild t2.
        assert_eq!(decide("t2", &running, None, None, None, true), Action::Nothing);
    }

    #[test]
    fn update_builds_wait_for_the_latest_desktop_tree_to_settle() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut quiet = BuildDebounce::default();
        assert!(!quiet.ready("t2", now));
        assert!(!quiet.ready("t2", now + Duration::from_secs(599)));
        assert!(quiet.ready("t2", now + Duration::from_secs(600)));
        // More landings during a build collapse into the newest tree.
        assert!(!quiet.ready("t3", now + Duration::from_secs(601)));
        assert!(!quiet.ready("t4", now + Duration::from_secs(900)));
        assert!(!quiet.ready("t4", now + Duration::from_secs(1499)));
        assert!(quiet.ready("t4", now + Duration::from_secs(1500)));
        // A busy host retry, or a landing outside desktop/, keeps its wait.
        assert!(quiet.ready("t4", now + Duration::from_secs(1800)));
    }

    /// A moment that would restart: staged, loaded, nothing typed or under
    /// way, the app in front and untouched for the idle time.
    fn quiet() -> RestartMoment {
        RestartMoment { staged: true, loaded: true, active: true, idle_for: AUTO_RESTART_IDLE_SECONDS, ..Default::default() }
    }

    #[test]
    fn restarts_in_front_once_untouched_for_five_seconds() {
        assert_eq!(AUTO_RESTART_IDLE_SECONDS, 5.);
        assert!(should_auto_restart(&quiet()));
        assert!(should_auto_restart(&RestartMoment { idle_for: 600., ..quiet() }));
        // Touched within the last five seconds: not yet.
        assert!(!should_auto_restart(&RestartMoment { idle_for: 4.9, ..quiet() }));
        assert!(!should_auto_restart(&RestartMoment { idle_for: 0., ..quiet() }));
    }

    #[test]
    fn the_wait_counts_down_and_starts_again_with_input() {
        let held = || "the message box in “Fix it” is sent or cleared".to_owned();
        let wait = |m: RestartMoment| restart_wait(&m, held);
        // In use: the full five. Then down to 1, and none once it's going.
        assert_eq!(wait(RestartMoment { idle_for: 0., ..quiet() }), Some(RestartWait::Countdown(5)));
        assert_eq!(wait(RestartMoment { idle_for: 0.4, ..quiet() }), Some(RestartWait::Countdown(5)));
        assert_eq!(wait(RestartMoment { idle_for: 2.5, ..quiet() }), Some(RestartWait::Countdown(3)));
        assert_eq!(wait(RestartMoment { idle_for: 4.9, ..quiet() }), Some(RestartWait::Countdown(1)));
        assert_eq!(wait(quiet()), None);
        assert_eq!(wait(RestartMoment { active: false, idle_for: 0., ..quiet() }), None);
        // Whatever holds it is said instead, in front or behind.
        for moment in [RestartMoment { has_drafts: true, ..quiet() }, RestartMoment { held: true, active: false, ..quiet() }, RestartMoment { other_window_input: true, idle_for: 0., ..quiet() }] {
            assert_eq!(wait(moment), Some(RestartWait::Held(held())));
        }
        assert_eq!(wait(RestartMoment { loaded: false, has_drafts: true, ..quiet() }), Some(RestartWait::Held("the threads have loaded".into())));
        assert_eq!(wait(RestartMoment { staged: false, idle_for: 0., ..quiet() }), None);
        // It says a wait exactly when it isn't restarting.
        for m in [quiet(), RestartMoment { idle_for: 1., ..quiet() }, RestartMoment { held: true, ..quiet() }, RestartMoment { active: false, ..quiet() }] {
            assert_eq!(wait(m).is_none(), should_auto_restart(&m), "{m:?}");
        }
    }

    #[test]
    fn a_field_holds_what_was_typed_since_it_was_saved() {
        assert!(!typed_since("", ""));
        assert!(!typed_since("Ship it", "Ship it "));
        assert!(typed_since("", "half a thought"));
        assert!(typed_since("Ship it", "Ship it by Friday"));
        // Cleared is a change too: goals being rewritten from nothing.
        assert!(typed_since("Ship it", ""));
    }

    #[test]
    fn a_web_page_holds_while_it_has_the_keyboard_or_was_used_lately() {
        assert!(web_page_holds(true, None));
        assert!(web_page_holds(false, Some(5.)));
        assert!(!web_page_holds(false, Some(WEB_PAGE_USED_SECONDS)));
        assert!(!web_page_holds(false, None));
    }

    #[test]
    fn restarts_straight_away_with_another_app_in_front() {
        assert!(should_auto_restart(&RestartMoment { active: false, idle_for: 0., ..quiet() }));
    }

    #[test]
    fn never_restarts_over_something_typed_or_under_way() {
        for (why, moment) in [
            ("unsent words", RestartMoment { has_drafts: true, ..quiet() }),
            ("a drag, a gesture, a menu, a sheet or a dialog", RestartMoment { held: true, ..quiet() }),
            ("typing in another window", RestartMoment { other_window_input: true, ..quiet() }),
            ("no build staged", RestartMoment { staged: false, ..quiet() }),
            ("the threads not in yet", RestartMoment { loaded: false, ..quiet() }),
        ] {
            assert!(!should_auto_restart(&moment), "in front: {why}");
            // Nor with another app in front: what's typed or open would still go.
            assert!(!should_auto_restart(&RestartMoment { active: false, idle_for: 0., ..moment }), "behind: {why}");
        }
    }

    #[test]
    fn the_tree_is_desktops_from_rev_parse() {
        assert_eq!(rev_parse_arguments("/repo"), ["-C", "/repo", "rev-parse", "refs/heads/main:desktop"]);
        assert_eq!(tree_from_rev_parse("aaa111\n"), "aaa111");
    }

    #[test]
    fn swap_puts_the_staged_bundle_in_place_and_keeps_the_old() {
        let dir = temp("swap");
        let install = bundle(&dir, "Threadbear GPUI.app", &build("aaa", "t1"));
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("bbb", "t2"));
        let old = dir.join(".Threadbear GPUI.app.old");
        let swapping = dir.join(".Threadbear GPUI.app.swapping");
        swap(&install, &next, &old, &swapping, "t2").unwrap();
        assert!(!swapping.exists(), "the note goes once it's done");
        assert_eq!(marker(&install).as_deref(), Some("bbb"));
        assert_eq!(marker(&old).as_deref(), Some("aaa"));
        assert!(!next.exists());
        // A swap that can't happen leaves the installed one where it is: the
        // exchange is one step, done or not.
        assert!(swap(&install, &dir.join("gone.app"), &old, &swapping, "t9").is_err());
        assert_eq!(marker(&install).as_deref(), Some("bbb"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_staged_swaps_only_a_different_build() {
        let dir = temp("apply");
        let install = bundle(&dir, "Threadbear GPUI.app", &build("aaa", "t1"));
        let old = dir.join(".Threadbear GPUI.app.old");
        let swapping = dir.join(".Threadbear GPUI.app.swapping");
        // The same tree as what's running: dropped, nothing swapped.
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("aab", "t1"));
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("aaa", "t1"))), Ok(None));
        assert!(!next.exists());
        assert_eq!(marker(&install).as_deref(), Some("aaa"));
        // A leftover from an earlier swap is cleared, and a new build goes in.
        bundle(&dir, ".Threadbear GPUI.app.old", &build("zzz", "t0"));
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("bbb", "t2"));
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("aaa", "t1"))).unwrap().map(|b| b.short), Some("bbb".to_owned()));
        assert_eq!(marker(&install).as_deref(), Some("bbb"));
        assert_eq!(marker(&old).as_deref(), Some("aaa"));
        // Nothing staged: nothing happens.
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("bbb", "t2"))), Ok(None));
        // A staged folder with no build.json is dropped.
        std::fs::create_dir_all(&next).unwrap();
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("bbb", "t2"))), Ok(None));
        assert!(!next.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_cut_short_is_finished_the_right_way() {
        let dir = temp("cut-short");
        let old = dir.join(".Threadbear GPUI.app.old");
        let swapping = dir.join(".Threadbear GPUI.app.swapping");
        // Killed after the exchange: the new one's in place, the old one's
        // left at next, and the note says t2 was coming in. The old one goes,
        // not back in, whatever the clocks said.
        let install = bundle(&dir, "Threadbear GPUI.app", &build("bbb", "t2"));
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("aaa", "t1"));
        std::fs::write(&swapping, "t2").unwrap();
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("bbb", "t2"))), Ok(None));
        assert_eq!(marker(&install).as_deref(), Some("bbb"));
        assert!(!next.exists() && !swapping.exists());
        // Killed before it: the note and the staged one agree, so it goes in.
        let _ = std::fs::remove_dir_all(&old);
        let install = bundle(&dir, "Threadbear GPUI.app", &build("aaa", "t1"));
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("bbb", "t2"));
        std::fs::write(&swapping, "t2").unwrap();
        assert_eq!(apply_staged(&install, &next, &old, &swapping, Some(&build("aaa", "t1"))).unwrap().map(|b| b.short), Some("bbb".to_owned()));
        assert_eq!(marker(&install).as_deref(), Some("bbb"));
        assert!(!swapping.exists());
        // Cut short after the exchange, and the old one can't be moved out of
        // the way (a file where .old goes): the note stays, nothing goes in.
        let install = bundle(&dir, "Threadbear GPUI.app", &build("ddd", "t4"));
        let next = bundle(&dir, ".Threadbear GPUI.app.next", &build("ccc", "t3"));
        let _ = std::fs::remove_dir_all(&old);
        std::fs::create_dir_all(old.join("in-the-way")).unwrap();
        std::fs::write(old.join("in-the-way").join("x"), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o500)).unwrap();
            std::fs::write(&swapping, "t4").unwrap();
            assert!(apply_staged(&install, &next, &old, &swapping, Some(&build("ddd", "t4"))).is_err());
            assert!(swapping.exists(), "the note stays until it's sorted");
            assert_eq!(marker(&install).as_deref(), Some("ddd"));
            std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Only a build of Mike's own repo reports to his Sentry.
    #[test]
    fn only_mikes_repo_counts_as_mikes() {
        for url in ["https://github.com/mikecann/threadbear.git", "https://github.com/MikeCann/threadbear", "git@github.com:mikecann/threadbear.git", "ssh://git@github.com/mikecann/threadbear", "https://x-access-token@github.com/mikecann/threadbear/"] {
            assert!(info(Some(url)).is_mikes_repo(), "{url}");
        }
        assert!(!info(Some("https://github.com/someone/threadbear.git")).is_mikes_repo());
        assert!(!info(Some("")).is_mikes_repo());
        assert!(!info(None).is_mikes_repo(), "built before origin was recorded, or no origin");
    }

    #[test]
    fn reads_what_the_bundle_script_writes() {
        let dir = std::env::temp_dir().join(format!("tb-build-info-{}", std::process::id()));
        let bundle = dir.join("Threadbear GPUI.app");
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        std::fs::write(
            BuildInfo::path_in(&bundle),
            r#"{"commit":"abc1234def","short":"abc1234","appTree":"t9","builtAt":"2026-11-11T10:00:00+00:00","checkout":"/repo","configuration":"release","origin":"git@github.com:mikecann/threadbear.git"}"#,
        )
        .unwrap();
        let read = BuildInfo::read(&bundle).unwrap();
        assert_eq!((read.short.as_str(), read.app_tree.as_str()), ("abc1234", "t9"));
        assert!(read.is_mikes_repo());
        assert_eq!(BuildInfo::read(&dir.join("missing.app")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bundle_is_found_from_its_executable() {
        let exe = Path::new("/Users/me/Applications/Threadbear GPUI.app/Contents/MacOS/threadbear-gpui");
        assert_eq!(bundle_of(exe).as_deref(), Some(Path::new("/Users/me/Applications/Threadbear GPUI.app")));
        assert_eq!(bundle_of(Path::new("/repo/desktop/target/debug/threadbear-gpui")), None);
    }
}
