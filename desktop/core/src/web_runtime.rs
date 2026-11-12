// The web runtime (docs/gpui-port.md, "Choice 1: the web view"): Chromium, as
// CEF's official build, fetched the first time something needs it rather than
// shipped inside the app. This is the part with no window and no network in
// it: which build this version of Threadbear runs, the folders it lives in,
// and the checks between the download and the unpacking. A download is held
// to its size, the SHA-1 Spotify publishes for it and a SHA-256 worked out
// from that same file, all before one file comes out of the archive, and the
// unpacking takes only the framework and refuses anything that would land
// outside it. desktop/web does the fetching and runs Chromium; desktop/gpui
// shows where it's got to.
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The CEF build this Threadbear runs. It has to be the one desktop/web's
/// `cef` crate was generated from (cef 156.3.0+156.0.15), or the two sides of
/// the C API disagree, so moving it means moving that pin too.
pub const CEF_VERSION: &str = "156.0.15+gcf671c0+chromium-156.0.8078.80";

/// Where the builds come from while Threadbear has no place of its own to
/// publish one: Spotify's CDN, which builds and hosts CEF's official binaries
/// (cef-builds.spotifycdn.com). Publishing a copy of our own, signed, waits
/// for Mike (docs/gpui-port.md).
pub const CDN: &str = "https://cef-builds.spotifycdn.com";

/// One system's build, pinned. `sha1` is what Spotify's index.json (and the
/// `.sha1` file beside the archive) says; `sha256` was worked out from the
/// file that matched it, on 11 Nov 2026, so a download is checked against a
/// hash that isn't SHA-1 alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Archive {
    /// Spotify's name for the system, as its index has it.
    pub platform: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha1: &'static str,
    pub sha256: &'static str,
}

/// The "minimal" distributions: the release build of the runtime with its
/// headers, no samples or debug build. Only the framework is kept.
pub const ARCHIVES: &[Archive] = &[
    Archive {
        platform: "macosarm64",
        file: "cef_binary_156.0.15+gcf671c0+chromium-156.0.8078.80_macosarm64_minimal.tar.bz2",
        bytes: 135_024_501,
        sha1: "bcdd1877edb300b0fb91d8f71c0b14aae256cd0f",
        sha256: "a27f1d856e84047657ed2a14caf355fb04480c4fe1963f04249a8a3184af1c30",
    },
    Archive {
        platform: "macosx64",
        file: "cef_binary_156.0.15+gcf671c0+chromium-156.0.8078.80_macosx64_minimal.tar.bz2",
        bytes: 141_647_289,
        sha1: "13756e3029307720fe00898d54aef8a64c9b4043",
        sha256: "e2aedb3cb41cc293ff2db81245f3462c61c49e13ac137f628872889c8a36c702",
    },
];

impl Archive {
    /// The archive's address. Spotify's own links write the `+`s as `%2B`.
    pub fn url(&self) -> String {
        format!("{CDN}/{}", self.file.replace('+', "%2B"))
    }

    pub fn pin(&self) -> Pin {
        Pin { bytes: self.bytes, sha1: self.sha1.to_owned(), sha256: self.sha256.to_owned(), top: self.file.trim_end_matches(".tar.bz2").to_owned() }
    }

    /// The download, in whole megabytes, as the prompt says it.
    pub fn megabytes(&self) -> u64 {
        (self.bytes + 500_000) / 1_000_000
    }
}

/// Spotify's name for the system this was built for, if there is one.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macosarm64"),
        ("macos", "x86_64") => Some("macosx64"),
        ("windows", "x86_64") => Some("windows64"),
        ("windows", "aarch64") => Some("windowsarm64"),
        ("linux", "x86_64") => Some("linux64"),
        ("linux", "aarch64") => Some("linuxarm64"),
        _ => None,
    }
}

/// This system's build, if this version of Threadbear has one. The Mac's
/// come first; Windows and Linux get theirs once the Mac's works
/// (docs/gpui-port/PARITY.md says what's untested there).
pub fn this_archive() -> Option<&'static Archive> {
    let platform = platform()?;
    ARCHIVES.iter().find(|a| a.platform == platform)
}

/// What a download and its unpacking are held to. Separate from `Archive` so
/// the tests can check small archives of their own the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub bytes: u64,
    pub sha1: String,
    pub sha256: String,
    /// The folder everything in the archive is under.
    pub top: String,
}

/// Where the runtime lives: the app's own data folder, never inside the app
/// (an app bundle that changes after it's signed stops being the one that was
/// signed, and a 330 MB app would be the bundled choice Mike turned down).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub root: PathBuf,
}

/// The framework's folder, inside the archive's `Release` and once unpacked.
pub const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
/// The bundle the runtime is laid out in, as an app bundle would hold it:
/// Contents/Frameworks with the framework and the helpers. Chromium is told
/// it's the main bundle. Its sandbox lets a helper read only inside that
/// bundle, so the framework has to be in it (a framework beside the helpers
/// and nothing round them was refused: "file system sandbox blocked
/// open()"), and a helper works out which process started it from the
/// bundle it's in. Not an `.app`, so nothing offers to open it.
pub const RUNTIME_BUNDLE: &str = "Threadbear Web.bundle";
/// The helper processes' name: CEF starts "<name>.app", "<name> (GPU).app",
/// and so on, from beside the framework.
pub const HELPER_NAME: &str = "Threadbear Helper";
/// The five helpers Chromium asks for on a Mac, with the end of each one's
/// bundle id (as CEF's own helper-Info.plist has them).
pub const HELPER_KINDS: [(&str, &str); 5] = [("", ""), (" (GPU)", ".gpu"), (" (Renderer)", ".renderer"), (" (Plugin)", ".plugin"), (" (Alerts)", ".alerts")];

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Layout { root: root.into() }
    }

    /// `THREADBEAR_WEB_RUNTIME_DIR` if it's set (the specs point it at a
    /// runtime installed once, or an empty folder), else the data folder's
    /// `web-runtime`. It doesn't follow THREADBEAR_HOME: the runtime belongs
    /// to the machine, not to whichever runner the app is talking to, so a
    /// dev runner doesn't mean another 135 MB download.
    pub fn default_root() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("THREADBEAR_WEB_RUNTIME_DIR").filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(dir));
        }
        if cfg!(windows) {
            let local = std::env::var_os("LOCALAPPDATA").filter(|d| !d.is_empty())?;
            return Some(PathBuf::from(local).join("Threadbear").join("web-runtime"));
        }
        // The runner keeps the Mac's folder names on every system for now
        // (INSTALL.md, "A runner on Linux"), so Linux has them too.
        let home = std::env::var_os("HOME").filter(|d| !d.is_empty())?;
        Some(PathBuf::from(home).join("Library").join("Application Support").join("Threadbear").join("web-runtime"))
    }

    /// One folder per CEF build, so a new Threadbear with a newer build never
    /// half-replaces the one an older copy may still be running.
    pub fn version_dir(&self) -> PathBuf {
        self.root.join(CEF_VERSION)
    }

    fn partial_dir(&self) -> PathBuf {
        self.root.join(format!("{CEF_VERSION}.partial"))
    }

    /// The bundle Chromium is told is its main one (RUNTIME_BUNDLE).
    pub fn runtime_bundle(&self) -> PathBuf {
        self.version_dir().join(RUNTIME_BUNDLE)
    }

    /// Where the framework and the helpers are, in that bundle.
    fn frameworks_in(dir: &Path) -> PathBuf {
        dir.join(RUNTIME_BUNDLE).join("Contents").join("Frameworks")
    }

    pub fn framework(&self) -> PathBuf {
        Self::frameworks_in(&self.version_dir()).join(FRAMEWORK)
    }

    /// The framework's library itself, which the app loads.
    pub fn framework_binary(&self) -> PathBuf {
        self.framework().join("Chromium Embedded Framework")
    }

    pub fn marker(&self) -> PathBuf {
        self.version_dir().join("installed.json")
    }

    /// Where a download goes while it's coming in.
    pub fn download_file(&self, archive_file: &str) -> PathBuf {
        self.root.join("download").join(format!("{archive_file}.part"))
    }

    /// Chromium's own folder: the panels' sign-ins and storage, kept between
    /// launches. The transcript's sealed pages keep nothing (desktop/web).
    pub fn profile(&self) -> PathBuf {
        self.root.join("profile")
    }

    /// Chromium's log, warnings and worse.
    pub fn log_file(&self) -> PathBuf {
        self.root.join("web-runtime.log")
    }

    /// The first helper; Chromium works out the others' paths from it.
    pub fn helper_executable(&self) -> PathBuf {
        helper_executable_in(&Self::frameworks_in(&self.version_dir()), "")
    }
}

fn helper_executable_in(dir: &Path, suffix: &str) -> PathBuf {
    let name = format!("{HELPER_NAME}{suffix}");
    dir.join(format!("{name}.app")).join("Contents").join("MacOS").join(name)
}

/// Where the runtime has got to on this machine, as the app shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// This version of Threadbear has no runtime for this system yet.
    Unavailable,
    /// Not installed.
    Missing,
    Downloading { received: u64, total: u64 },
    /// Checked, and coming out of the archive.
    Unpacking,
    Ready,
    Failed(String),
}

impl State {
    /// What it's called in the app's dump, for the specs.
    pub fn name(&self) -> &'static str {
        match self {
            State::Unavailable => "unavailable",
            State::Missing => "missing",
            State::Downloading { .. } => "downloading",
            State::Unpacking => "unpacking",
            State::Ready => "ready",
            State::Failed(_) => "failed",
        }
    }

    /// How far a download has got, 0 to 100.
    pub fn percent(&self) -> Option<u8> {
        match self {
            State::Downloading { received, total } if *total > 0 => Some(((received.min(total) * 100) / total) as u8),
            _ => None,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self, State::Downloading { .. } | State::Unpacking)
    }

    /// The state in a few words: Settings' line and the prompt's progress.
    pub fn words(&self) -> String {
        match self {
            State::Unavailable => "Not available for this system yet".to_owned(),
            State::Missing => "Not installed".to_owned(),
            State::Downloading { received, total } => format!("Downloading, {} of {} MB", received / 1_000_000, (total + 500_000) / 1_000_000),
            State::Unpacking => "Unpacking".to_owned(),
            State::Ready => "Installed".to_owned(),
            State::Failed(why) => format!("Couldn't install it: {why}"),
        }
    }
}

/// Whether a download stopped because it was asked to, or failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    Cancelled,
    Failed(String),
}

impl InstallError {
    pub fn message(&self) -> String {
        match self {
            InstallError::Cancelled => "it was stopped".to_owned(),
            InstallError::Failed(why) => why.clone(),
        }
    }
}

fn failed(why: impl Into<String>) -> InstallError {
    InstallError::Failed(why.into())
}

/// A file system error in plain words, with a full disk said as such.
fn disk(what: &str, error: io::Error) -> InstallError {
    // ENOSPC on the Mac and Linux, ERROR_DISK_FULL on Windows.
    if error.raw_os_error() == Some(28) || error.raw_os_error() == Some(112) {
        return failed(format!("there isn't room on the disk ({what}). It needs about 500 MB free"));
    }
    failed(format!("{what}: {error}"))
}

/// Copies a download to `part` while it's hashed and counted, and keeps it
/// only if it's exactly what `pin` says: the size, Spotify's SHA-1 and the
/// SHA-256. Anything else is deleted and said why; a download that's longer
/// than it should be stops there. Nothing is unpacked here.
pub fn receive(body: &mut dyn Read, part: &Path, pin: &Pin, progress: &mut dyn FnMut(u64), cancel: &AtomicBool) -> Result<(), InstallError> {
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).map_err(|e| disk("couldn't make the download folder", e))?;
    }
    let result = (|| {
        let mut file = std::fs::File::create(part).map_err(|e| disk("couldn't start the download", e))?;
        let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
        let mut buffer = vec![0u8; 256 * 1024];
        let mut received: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            let n = match body.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(failed(format!("the download stopped: {e}"))),
            };
            received += n as u64;
            if received > pin.bytes {
                return Err(failed("the download was bigger than the pinned build, so it isn't that build"));
            }
            sha1.update(&buffer[..n]);
            sha256.update(&buffer[..n]);
            file.write_all(&buffer[..n]).map_err(|e| disk("couldn't save the download", e))?;
            progress(received);
        }
        if received != pin.bytes {
            return Err(failed(format!("the download ended early ({} of {} MB)", received / 1_000_000, pin.bytes / 1_000_000)));
        }
        file.sync_all().map_err(|e| disk("couldn't save the download", e))?;
        if hex(&sha1.finalize()) != pin.sha1.to_ascii_lowercase() || hex(&sha256.finalize()) != pin.sha256.to_ascii_lowercase() {
            return Err(failed("the download didn't match the pinned build's hash, so it wasn't used"));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

/// Hashes a whole open file from where it is, for the check just before
/// it's unpacked from that same handle.
fn file_hashes(file: &mut std::fs::File) -> io::Result<(u64, String, String)> {
    let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
    let mut buffer = vec![0u8; 256 * 1024];
    let mut len = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        len += n as u64;
        sha1.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
    }
    Ok((len, hex(&sha1.finalize()), hex(&sha256.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Installs from an archive on disk: hashed again first, whoever handed it
/// over (a download just checked, or a copy a spec points at), so nothing is
/// ever unpacked from a file that doesn't match the pin. Then the framework
/// alone comes out into a `.partial` folder, the marker goes in last, and the
/// folder is renamed into place whole. Other builds' folders go after that.
pub fn install_from_file(archive: &Path, pin: &Pin, layout: &Layout) -> Result<(), InstallError> {
    install_checked(archive, pin, layout, || {})
}

/// `install_from_file`, with `between` run after the hash is checked and
/// before anything's unpacked, for the test that swaps the file there.
fn install_checked(archive: &Path, pin: &Pin, layout: &Layout, between: impl FnOnce()) -> Result<(), InstallError> {
    // One handle, hashed and then unpacked from its start again: opened a
    // second time by name, a file swapped in between would be unpacked
    // under the pinned hash's name (Codex review, 11 Nov).
    let mut file = std::fs::File::open(archive).map_err(|e| failed(format!("couldn't open the download: {e}")))?;
    let (len, sha1, sha256) = file_hashes(&mut file).map_err(|e| failed(format!("couldn't read the download: {e}")))?;
    if len != pin.bytes || sha1 != pin.sha1.to_ascii_lowercase() || sha256 != pin.sha256.to_ascii_lowercase() {
        return Err(failed("the archive didn't match the pinned build's hash, so it wasn't unpacked"));
    }
    between();
    file.seek(SeekFrom::Start(0)).map_err(|e| failed(format!("couldn't read the download: {e}")))?;
    let partial = layout.partial_dir();
    if partial.exists() {
        std::fs::remove_dir_all(&partial).map_err(|e| disk("couldn't clear an earlier try", e))?;
    }
    std::fs::create_dir_all(&partial).map_err(|e| disk("couldn't make the runtime's folder", e))?;
    let result = unpack_framework(file, pin, &Layout::frameworks_in(&partial)).and_then(|()| {
        let marker = serde_json::json!({ "version": CEF_VERSION, "sha256": pin.sha256.to_ascii_lowercase() });
        std::fs::write(partial.join("installed.json"), marker.to_string()).map_err(|e| disk("couldn't finish the install", e))
    });
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    let dir = layout.version_dir();
    if dir.exists() {
        // A folder without a good marker: a copy cut off before this check.
        std::fs::remove_dir_all(&dir).map_err(|e| disk("couldn't replace an unfinished copy", e))?;
    }
    std::fs::rename(&partial, &dir).map_err(|e| disk("couldn't put the runtime in place", e))?;
    remove_other_builds(layout);
    Ok(())
}

/// Older builds' folders, and anything half-made. The profile, the log and
/// the download folder stay.
fn remove_other_builds(layout: &Layout) {
    let Ok(entries) = std::fs::read_dir(&layout.root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let looks_like_build = name.chars().next().is_some_and(|c| c.is_ascii_digit()) && name.contains("chromium-");
        if looks_like_build && name != CEF_VERSION && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Whether the pinned build is installed: its marker names this build and
/// hash, and the framework's library is there.
pub fn is_installed(layout: &Layout, pin: &Pin) -> bool {
    let Ok(text) = std::fs::read_to_string(layout.marker()) else { return false };
    let Ok(marker) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    marker["version"] == CEF_VERSION && marker["sha256"].as_str() == Some(&pin.sha256.to_ascii_lowercase()) && layout.framework_binary().is_file()
}

/// The part of an entry's path under `<top>/Release/<framework>`, checked:
/// only plain names, none of `..`, `/` at the start or a drive. None for
/// anything else in the archive (headers, the wrapper's source), which is
/// skipped; an entry that tries to climb out is an error.
fn framework_path(entry: &Path, top: &str) -> Result<Option<PathBuf>, InstallError> {
    let mut parts = Vec::new();
    for component in entry.components() {
        match component {
            Component::Normal(name) => parts.push(name.to_owned()),
            Component::CurDir => {}
            _ => return Err(failed(format!("the archive has a path that leaves its folder ({})", entry.display()))),
        }
    }
    let [first, release, framework, rest @ ..] = parts.as_slice() else { return Ok(None) };
    if first.to_str() != Some(top) || release != "Release" || framework != FRAMEWORK {
        return Ok(None);
    }
    Ok(Some(rest.iter().collect()))
}

/// The framework out of a checked archive (`file`, at its start) into
/// `dest`: its files, with their executable bit but no set-id bits or
/// owners, and its folders. Nothing else. CEF's framework has no links
/// (156.0.15's two archives have none, of either kind), and a link is how an
/// archive gets a later entry written somewhere else: checks of where links
/// point were got round twice in review (Codex, 11 Nov: `a -> .`, then
/// `escape -> a/..`, then a file written to `escape/planted-file`; then two
/// spellings of one name, which the Mac's disk takes as the same, written
/// through a link made a moment before). With none made, every entry lands
/// where its own checked path says, inside the framework.
fn unpack_framework(file: std::fs::File, pin: &Pin, dest: &Path) -> Result<(), InstallError> {
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(io::BufReader::new(file)));
    let root = dest.join(FRAMEWORK);
    let mut files = 0usize;
    let entries = tar.entries().map_err(|e| failed(format!("couldn't read the archive: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| failed(format!("couldn't read the archive: {e}")))?;
        let path = entry.path().map_err(|e| failed(format!("couldn't read a name in the archive: {e}")))?.into_owned();
        let Some(inside) = framework_path(&path, &pin.top)? else { continue };
        let out = root.join(&inside);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| disk("couldn't unpack it", e))?;
        } else if kind.is_file() {
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| disk("couldn't unpack it", e))?;
            }
            let mut target = std::fs::File::create(&out).map_err(|e| disk("couldn't unpack it", e))?;
            io::copy(&mut entry, &mut target).map_err(|e| disk("couldn't unpack it", e))?;
            set_mode(&out, entry.header().mode().unwrap_or(0o644));
            files += 1;
        } else if kind.is_symlink() || kind.is_hard_link() {
            return Err(failed(format!("the archive has a link in the framework ({}), which CEF's own build doesn't, so it wasn't used", inside.display())));
        } else {
            return Err(failed(format!("the archive has something that isn't a file or folder ({})", path.display())));
        }
    }
    if files == 0 {
        return Err(failed("the archive had no framework in it"));
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    // Executable where the archive says so, writable only by you, and never
    // set-user or set-group.
    let mode = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

/// The Mac's helper apps beside the framework, each a copy of the app's own
/// `threadbear-web-helper` with an Info.plist saying it's a background app
/// (no Dock icon) and its bundle id. Made again whenever that binary changes
/// (an update of the app), and left alone otherwise: a stamp holds the
/// binary's SHA-256. Returns the first helper's path, for Chromium.
pub fn make_helpers(layout: &Layout, binary: &Path, bundle_id: &str) -> Result<PathBuf, String> {
    let bytes = std::fs::read(binary).map_err(|e| format!("couldn't read the web helper ({}): {e}", binary.display()))?;
    // The binary and the ids it's given: either changing means making them again.
    let stamped = format!("{}\n{bundle_id}", hex(&Sha256::digest(&bytes)));
    let stamp = layout.version_dir().join("helpers.stamp");
    let dir = Layout::frameworks_in(&layout.version_dir());
    let all_there = HELPER_KINDS.iter().all(|(suffix, _)| helper_executable_in(&dir, suffix).is_file());
    if all_there && std::fs::read_to_string(&stamp).is_ok_and(|s| s == stamped) {
        return Ok(layout.helper_executable());
    }
    // The bundle round them, which Chromium takes for the app's: its id is
    // what a helper and the app agree on to find each other.
    let bundle_id = format!("{bundle_id}.web");
    let contents = layout.runtime_bundle().join("Contents");
    std::fs::create_dir_all(&contents).map_err(|e| format!("couldn't make the runtime's bundle: {e}"))?;
    std::fs::write(contents.join("Info.plist"), runtime_bundle_plist(&bundle_id)).map_err(|e| format!("couldn't write the runtime's Info.plist: {e}"))?;
    for (suffix, id_end) in HELPER_KINDS {
        let name = format!("{HELPER_NAME}{suffix}");
        let exe = helper_executable_in(&dir, suffix);
        let contents = exe.parent().and_then(Path::parent).ok_or("a helper's folder")?.to_owned();
        std::fs::create_dir_all(exe.parent().ok_or("a helper's folder")?).map_err(|e| format!("couldn't make {name}: {e}"))?;
        // Written beside and renamed over, so a helper another copy of the
        // app is running from keeps its old file until it's done.
        let fresh = exe.with_extension("new");
        std::fs::write(&fresh, &bytes).map_err(|e| format!("couldn't write {name}: {e}"))?;
        set_mode(&fresh, 0o755);
        std::fs::rename(&fresh, &exe).map_err(|e| format!("couldn't write {name}: {e}"))?;
        std::fs::write(contents.join("Info.plist"), helper_plist(&name, &format!("{bundle_id}.helper{id_end}"))).map_err(|e| format!("couldn't write {name}'s Info.plist: {e}"))?;
    }
    std::fs::write(&stamp, &stamped).map_err(|e| format!("couldn't finish the helpers: {e}"))?;
    Ok(layout.helper_executable())
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// RUNTIME_BUNDLE's Info.plist: a plain bundle with an id, nothing to run.
pub fn runtime_bundle_plist(bundle_id: &str) -> String {
    let bundle_id = xml(bundle_id);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>{bundle_id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Threadbear Web</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
</dict>
</plist>
"#
    )
}

/// CEF's helper-Info.plist, filled in. LSUIElement keeps them out of the
/// Dock; MallocNanoZone is off as CEF's own template has it.
pub fn helper_plist(name: &str, bundle_id: &str) -> String {
    let (name, bundle_id) = (xml(name), xml(bundle_id));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>{name}</string>
  <key>CFBundleExecutable</key><string>{name}</string>
  <key>CFBundleIdentifier</key><string>{bundle_id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  <key>LSFileQuarantineEnabled</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><string>1</string>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-web-runtime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pin_for(bytes: &[u8], top: &str) -> Pin {
        Pin { bytes: bytes.len() as u64, sha1: hex(&Sha1::digest(bytes)), sha256: hex(&Sha256::digest(bytes)), top: top.to_owned() }
    }

    const TOP: &str = "cef_binary_test_macosarm64_minimal";

    /// A small archive shaped like CEF's: the framework under Release (files
    /// and folders, no links, as the real one), with the headers and a
    /// static library beside it that aren't wanted.
    fn archive(extra: impl FnOnce(&mut tar::Builder<bzip2::write::BzEncoder<Vec<u8>>>)) -> Vec<u8> {
        let mut builder = tar::Builder::new(bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast()));
        let mut add = |path: &str, data: &[u8], mode: u32| {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(mode);
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, path, data).unwrap();
        };
        let fw = format!("{TOP}/Release/{FRAMEWORK}");
        add(&format!("{fw}/Chromium Embedded Framework"), b"the library", 0o4755);
        add(&format!("{fw}/Resources/icudtl.dat"), b"data", 0o644);
        add(&format!("{fw}/Libraries/libcef_sandbox.dylib"), b"sandbox", 0o755);
        add(&format!("{TOP}/Release/cef_sandbox.a"), b"not wanted", 0o644);
        add(&format!("{TOP}/include/cef_app.h"), b"not wanted", 0o644);
        extra(&mut builder);
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// An entry written with a raw name, which tar's own builder refuses to
    /// write, as a hostile archive would have it.
    fn raw_entry(builder: &mut tar::Builder<bzip2::write::BzEncoder<Vec<u8>>>, name: &str, kind: tar::EntryType, link: &str) {
        let mut header = tar::Header::new_old();
        let bytes = header.as_old_mut();
        bytes.name[..name.len()].copy_from_slice(name.as_bytes());
        bytes.linkname[..link.len()].copy_from_slice(link.as_bytes());
        header.set_entry_type(kind);
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, io::empty()).unwrap();
    }

    #[test]
    fn a_download_that_does_not_match_is_never_kept() {
        let dir = temp("mismatch");
        let part = dir.join("download/x.part");
        let good = b"the real build".to_vec();
        let pin = pin_for(&good, TOP);
        let no = AtomicBool::new(false);
        let changed = b"the real buile".to_vec();
        let err = receive(&mut changed.as_slice(), &part, &pin, &mut |_| {}, &no).unwrap_err();
        assert!(err.message().contains("didn't match"), "{err:?}");
        assert!(!part.exists(), "a download that didn't match is deleted");
        let short = &good[..5];
        assert!(receive(&mut &short[..], &part, &pin, &mut |_| {}, &no).unwrap_err().message().contains("ended early"));
        let long = [good.as_slice(), b"!"].concat();
        assert!(receive(&mut long.as_slice(), &part, &pin, &mut |_| {}, &no).unwrap_err().message().contains("bigger"));
        let mut seen = 0;
        receive(&mut good.as_slice(), &part, &pin, &mut |n| seen = n, &no).unwrap();
        assert_eq!(seen, good.len() as u64);
        assert_eq!(std::fs::read(&part).unwrap(), good);
        let stop = AtomicBool::new(true);
        assert_eq!(receive(&mut good.as_slice(), &part, &pin, &mut |_| {}, &stop).unwrap_err(), InstallError::Cancelled);
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpacks_only_the_framework_and_marks_it_last() {
        let dir = temp("unpack");
        let data = archive(|_| {});
        let file = dir.join("a.tar.bz2");
        std::fs::write(&file, &data).unwrap();
        let pin = pin_for(&data, TOP);
        let layout = Layout::new(dir.join("runtime"));
        assert!(!is_installed(&layout, &pin));
        // An older build's folder goes once this one's in place.
        let old = layout.root.join("150.0.1+gabc+chromium-150.0.1.1");
        std::fs::create_dir_all(&old).unwrap();
        install_from_file(&file, &pin, &layout).unwrap();
        assert!(is_installed(&layout, &pin));
        assert!(!old.exists(), "the older build was removed");
        let fw = layout.framework();
        assert_eq!(std::fs::read(fw.join("Chromium Embedded Framework")).unwrap(), b"the library");
        assert!(fw.join("Libraries/libcef_sandbox.dylib").is_file());
        assert!(!layout.version_dir().join("cef_sandbox.a").exists() && !layout.version_dir().join("include").exists(), "only the framework comes out");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(fw.join("Chromium Embedded Framework")).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, 0o755, "executable, and the set-user bit dropped");
        }
        assert!(!layout.root.join(format!("{CEF_VERSION}.partial")).exists());
        // Another pin (a newer Threadbear's) doesn't count this one installed.
        assert!(!is_installed(&layout, &Pin { sha256: "0".repeat(64), ..pin.clone() }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checks_the_file_again_before_unpacking() {
        let dir = temp("recheck");
        let data = archive(|_| {});
        let file = dir.join("a.tar.bz2");
        std::fs::write(&file, &data).unwrap();
        let mut pin = pin_for(&data, TOP);
        pin.sha256 = "1".repeat(64);
        let layout = Layout::new(dir.join("runtime"));
        let err = install_from_file(&file, &pin, &layout).unwrap_err();
        assert!(err.message().contains("wasn't unpacked"), "{err:?}");
        assert!(!layout.version_dir().exists() && !layout.root.join(format!("{CEF_VERSION}.partial")).exists(), "nothing was written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_anything_that_would_land_outside_the_framework() {
        let cases: Vec<(&str, Box<dyn Fn(&mut tar::Builder<bzip2::write::BzEncoder<Vec<u8>>>)>)> = vec![
            ("climbs out", Box::new(|b| raw_entry(b, &format!("{TOP}/Release/{FRAMEWORK}/../../../escaped"), tar::EntryType::Regular, ""))),
            ("absolute link", Box::new(|b| raw_entry(b, &format!("{TOP}/Release/{FRAMEWORK}/evil"), tar::EntryType::Symlink, "/etc/passwd"))),
            ("link that climbs", Box::new(|b| raw_entry(b, &format!("{TOP}/Release/{FRAMEWORK}/Resources/evil"), tar::EntryType::Symlink, "../../../../outside"))),
            ("hard link", Box::new(|b| raw_entry(b, &format!("{TOP}/Release/{FRAMEWORK}/hard"), tar::EntryType::Link, "Chromium Embedded Framework"))),
        ];
        for (what, extra) in cases {
            let dir = temp("hostile");
            let data = archive(|b| extra(b));
            let file = dir.join("a.tar.bz2");
            std::fs::write(&file, &data).unwrap();
            let pin = pin_for(&data, TOP);
            let layout = Layout::new(dir.join("runtime"));
            assert!(install_from_file(&file, &pin, &layout).is_err(), "{what} was let through");
            assert!(!layout.version_dir().exists() && !layout.root.join(format!("{CEF_VERSION}.partial")).exists(), "{what}: nothing kept");
            assert!(!dir.join("escaped").exists() && !dir.join("outside").exists());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Whether anything at all is at `path`, a link included (`exists`
    /// follows a link, and calls one to nowhere missing).
    fn anything_at(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// Unpacks `data` straight into a folder of its own, as an install
    /// would into `.partial`, so what lands beside the framework can be seen
    /// (an install deletes `.partial` when it fails).
    fn unpack_into(name: &str, data: &[u8]) -> (PathBuf, Result<(), InstallError>) {
        let dir = temp(name);
        let file = dir.join("a.tar.bz2");
        std::fs::write(&file, data).unwrap();
        let dest = dir.join("Frameworks");
        std::fs::create_dir_all(&dest).unwrap();
        let result = unpack_framework(std::fs::File::open(&file).unwrap(), &pin_for(data, TOP), &dest);
        (dir, result)
    }

    #[test]
    fn a_link_through_a_link_cannot_plant_a_file_outside() {
        // Codex's three entries (11 Nov): each link looked as if it stayed
        // inside, and made as they came, the file's create followed `escape`
        // to `a/..`, `a` to `.`, and so up past the framework's top.
        let fw = format!("{TOP}/Release/{FRAMEWORK}");
        let data = archive(|b| {
            raw_entry(b, &format!("{fw}/a"), tar::EntryType::Symlink, ".");
            raw_entry(b, &format!("{fw}/escape"), tar::EntryType::Symlink, "a/..");
            raw_entry(b, &format!("{fw}/escape/planted-file"), tar::EntryType::Regular, "");
        });
        let (dir, result) = unpack_into("through-links", &data);
        assert!(result.is_err(), "it was unpacked");
        assert!(!anything_at(&dir.join("Frameworks/planted-file")) && !anything_at(&dir.join("planted-file")), "a file landed outside the framework");
        let root = dir.join(format!("Frameworks/{FRAMEWORK}"));
        assert!(!anything_at(&root.join("a")) && !anything_at(&root.join("escape")), "a link was made");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_link_of_any_kind_is_made() {
        let fw = format!("{TOP}/Release/{FRAMEWORK}");
        // A link with nothing wrong with its target, alone or with things
        // written through it, either way round; a link under another link;
        // and, from Codex's second look (11 Nov), names the Mac's disk takes
        // as one: Lib and lib, and é written precomposed and as e and an
        // accent.
        let cases: Vec<(&str, Box<dyn Fn(&mut tar::Builder<bzip2::write::BzEncoder<Vec<u8>>>)>)> = vec![
            ("a plain link", Box::new(|b| raw_entry(b, &format!("{fw}/Versions/Current"), tar::EntryType::Symlink, "A"))),
            ("a hard link", Box::new(|b| raw_entry(b, &format!("{fw}/hard"), tar::EntryType::Link, "Chromium Embedded Framework"))),
            ("link then file", Box::new(|b| {
                raw_entry(b, &format!("{fw}/lib"), tar::EntryType::Symlink, "Libraries");
                raw_entry(b, &format!("{fw}/lib/planted-file"), tar::EntryType::Regular, "");
            })),
            ("file then link", Box::new(|b| {
                raw_entry(b, &format!("{fw}/lib/planted-file"), tar::EntryType::Regular, "");
                raw_entry(b, &format!("{fw}/lib"), tar::EntryType::Symlink, "Libraries");
            })),
            ("link under a link", Box::new(|b| {
                raw_entry(b, &format!("{fw}/lib"), tar::EntryType::Symlink, "Libraries");
                raw_entry(b, &format!("{fw}/lib/planted-link"), tar::EntryType::Symlink, "Resources");
            })),
            ("link and file in another case", Box::new(|b| {
                raw_entry(b, &format!("{fw}/Lib"), tar::EntryType::Symlink, "Libraries");
                raw_entry(b, &format!("{fw}/lib/planted-file"), tar::EntryType::Regular, "");
            })),
            ("two spellings of one name", Box::new(|b| {
                raw_entry(b, &format!("{fw}/\u{e9}"), tar::EntryType::Symlink, ".");
                // (Short, to fit an old header's hundred bytes.)
                raw_entry(b, &format!("{fw}/e\u{301}/new/link"), tar::EntryType::Symlink, ".");
            })),
        ];
        for (what, extra) in cases {
            let data = archive(|b| extra(b));
            let (dir, result) = unpack_into("links", &data);
            assert!(result.as_ref().is_err_and(|e| e.message().contains("link")), "{what} was unpacked: {result:?}");
            assert_eq!(links_under(&dir), Vec::<PathBuf>::new(), "{what}: a link was made");
            let beside: Vec<_> = std::fs::read_dir(dir.join("Frameworks")).unwrap().flatten().map(|e| e.file_name()).collect();
            assert!(beside.iter().all(|name| name == FRAMEWORK), "{what}: {beside:?} beside the framework");
            assert!(!anything_at(&dir.join(format!("Frameworks/{FRAMEWORK}/Libraries/planted-file"))), "{what}: a file was written through a link");
            assert!(!anything_at(&dir.join(format!("Frameworks/{FRAMEWORK}/hard"))), "{what}: a hard link was made");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Every link under `dir`, found without following any.
    fn links_under(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let kind = entry.file_type().unwrap();
            if kind.is_symlink() {
                found.push(entry.path());
            } else if kind.is_dir() {
                found.extend(links_under(&entry.path()));
            }
        }
        found
    }

    #[test]
    fn unpacks_the_file_it_hashed_even_if_its_name_is_swapped_after() {
        let dir = temp("swap");
        let good = archive(|_| {});
        let file = dir.join("a.tar.bz2");
        std::fs::write(&file, &good).unwrap();
        // The same shape, with another library over the first.
        let other = archive(|b| {
            let mut header = tar::Header::new_gnu();
            header.set_size(10);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::Regular);
            b.append_data(&mut header, format!("{TOP}/Release/{FRAMEWORK}/Chromium Embedded Framework"), &b"swapped in"[..]).unwrap();
        });
        let layout = Layout::new(dir.join("runtime"));
        install_checked(&file, &pin_for(&good, TOP), &layout, || {
            // Between the hash and the unpacking, another file takes its name.
            let swapped = dir.join("swapped.tar.bz2");
            std::fs::write(&swapped, &other).unwrap();
            std::fs::rename(&swapped, &file).unwrap();
        })
        .unwrap();
        assert_eq!(std::fs::read(layout.framework().join("Chromium Embedded Framework")).unwrap(), b"the library", "what was hashed is what was unpacked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn helpers_are_made_once_per_binary() {
        let dir = temp("helpers");
        let layout = Layout::new(dir.join("runtime"));
        let binary = dir.join("threadbear-web-helper");
        std::fs::write(&binary, b"helper v1").unwrap();
        let first = make_helpers(&layout, &binary, "com.example.app").unwrap();
        assert_eq!(first, layout.helper_executable());
        // In the bundle Chromium takes for the app's, beside where the
        // framework goes, as an app bundle would hold them.
        assert!(first.starts_with(layout.runtime_bundle().join("Contents/Frameworks")));
        let bundle = std::fs::read_to_string(layout.runtime_bundle().join("Contents/Info.plist")).unwrap();
        assert!(bundle.contains("<string>com.example.app.web</string>"), "{bundle}");
        for (suffix, id_end) in HELPER_KINDS {
            let exe = helper_executable_in(&Layout::frameworks_in(&layout.version_dir()), suffix);
            assert_eq!(std::fs::read(&exe).unwrap(), b"helper v1");
            // ../../.. from a helper's MacOS folder is where the framework
            // is, as CEF's loader and sandbox look for it.
            let up = exe.ancestors().nth(4).unwrap();
            assert_eq!(up, layout.framework().parent().unwrap(), "{}", exe.display());
            let plist = std::fs::read_to_string(exe.parent().unwrap().parent().unwrap().join("Info.plist")).unwrap();
            assert!(plist.contains("<key>LSUIElement</key><string>1</string>"));
            assert!(plist.contains(&format!("<string>com.example.app.web.helper{id_end}</string>")), "{plist}");
            assert!(plist.contains(&format!("<string>{HELPER_NAME}{suffix}</string>")));
        }
        // Same binary: left alone. A new one (the app updated): replaced.
        let exe = layout.helper_executable();
        let before = std::fs::metadata(&exe).unwrap().modified().unwrap();
        make_helpers(&layout, &binary, "com.example.app").unwrap();
        assert_eq!(std::fs::metadata(&exe).unwrap().modified().unwrap(), before);
        std::fs::write(&binary, b"helper v2").unwrap();
        make_helpers(&layout, &binary, "com.example.app").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"helper v2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn says_where_it_has_got_to() {
        assert_eq!(State::Missing.words(), "Not installed");
        let s = State::Downloading { received: 45_200_000, total: 135_024_501 };
        assert_eq!(s.words(), "Downloading, 45 of 135 MB");
        assert_eq!(s.percent(), Some(33));
        assert!(s.busy());
        assert_eq!(State::Failed("no network".into()).words(), "Couldn't install it: no network");
        assert_eq!(ARCHIVES[0].megabytes(), 135);
        assert!(ARCHIVES[0].url().ends_with("cef_binary_156.0.15%2Bgcf671c0%2Bchromium-156.0.8078.80_macosarm64_minimal.tar.bz2"));
        for archive in ARCHIVES {
            assert!(archive.file.contains(CEF_VERSION), "{} is the pinned build", archive.file);
            assert_eq!(archive.sha1.len(), 40);
            assert_eq!(archive.sha256.len(), 64, "{}'s SHA-256 is pinned", archive.platform);
        }
    }
}
