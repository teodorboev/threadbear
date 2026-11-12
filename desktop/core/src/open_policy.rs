// What the GPUI app hands to the system, and when it asks first: the rules of
// the Swift app's OpenGate and LinkPolicy (SECURITY.md, "Your Mac and phone
// from links and files"), with no UI in them so they can be tested anywhere.
// `desktop/gpui/src/open_gate.rs` is the only place that acts on them, and
// `tests/open_gate_source.rs` fails on an opener anywhere else.
//
// A link or path can come from an agent's words, a cloned repo's skill, a
// patch, the backend or another Mac's runner, and the system opener runs
// whatever app it has for the target: a `file://` URL to a `.command` runs it
// in Terminal, an `.app` launches, and a `#` left unescaped in a URL built by
// pasting a path in makes `Inspect.app#help/SKILL.md` open `Inspect.app`
// (Daybreak, 9 Nov 2026, findings 58, 59 and 64). So:
//
// - web addresses (http, https with a host) open in the browser;
// - any other scheme starts its app only once Mike says yes to that link;
// - a file is looked at after its symlinks are followed, and that real file
//   is what opens, through a URL built here from the path, never by pasting
//   a path into `file://`. A known kind of document opens; anything else (a
//   program, a bundle, a script, a kind it can't place, a document whose app
//   runs it) asks first, and another Mac's path never opens here.
use std::path::{Path, PathBuf};

/// Whose link or file it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// This runner's thread, a pin, a folder Mike's browsing: a program asks.
    ThisMac,
    /// A thread on another runner. Its paths name files on that machine, so
    /// none opens here; its web links still do, and other links ask.
    OtherMac,
}

/// What a link is, for opening it (`LinkPolicy.Kind`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// http or https with a host: the browser, without asking.
    Web(String),
    /// A `file:` URL or a bare path: its decoded path, without the ?query or
    /// #fragment, which never name part of the file.
    File(String),
    /// Any other scheme (`mailto:`, `slack:`): it starts another app, which
    /// acts on what the link says, so only once Mike says yes to it.
    Ask(String),
    /// Nothing that can be opened.
    Refuse,
}

/// The longest link looked at (`LinkPolicy.web`'s limit).
const MAX_LINK: usize = 8_192;

/// What `url` is. A link with no scheme is a path, relative or not; the
/// caller resolves a relative one against the thread's folder.
pub fn link_kind(url: &str) -> LinkKind {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_LINK || url.chars().any(char::is_control) {
        return LinkKind::Refuse;
    }
    match scheme(url) {
        // A link to a place in the page itself (`#usage`) names no file.
        None => match url_path(url) {
            path if path.is_empty() => LinkKind::Refuse,
            path => LinkKind::File(path),
        },
        Some(s) if s == "http" || s == "https" => {
            if crate::reducer::event::is_web_address(url) {
                LinkKind::Web(url.to_owned())
            } else {
                LinkKind::Refuse
            }
        }
        Some(s) if s == "file" => {
            let rest = &url[5..];
            // file:///a/b and file:/a/b name /a/b; file://host/a names a
            // file on another machine, which isn't opened from here.
            let path = if let Some(after) = rest.strip_prefix("//") {
                let (host, path) = after.find('/').map_or((after, ""), |i| (&after[..i], &after[i..]));
                if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                    return LinkKind::Refuse;
                }
                path
            } else {
                rest
            };
            let path = url_path(path);
            #[cfg(windows)]
            let path = if path.starts_with('/') && crate::reducer::text::is_windows_path(&path[1..]) { path[1..].to_owned() } else { path };
            if path.is_empty() {
                LinkKind::Refuse
            } else {
                LinkKind::File(path)
            }
        }
        // A Windows path's drive letter reads as a one-letter scheme.
        Some(s) if s.len() == 1 && url[2..].starts_with(['/', '\\']) => LinkKind::File(url.to_owned()),
        // A drive-relative path is still a file, never an app's scheme.
        // Its missing root (or an embedded device name) is refused below.
        Some(s) if cfg!(windows) && s.len() == 1 => LinkKind::File(url.to_owned()),
        Some(_) => LinkKind::Ask(url.to_owned()),
    }
}

/// The scheme of a URL, lowercased, or None for a path.
fn scheme(url: &str) -> Option<String> {
    let (s, _) = url.split_once(':')?;
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
        return None;
    }
    Some(s.to_ascii_lowercase())
}

/// The path part of a URL: cut at the first `?` or `#` before anything is
/// decoded, as a URL parser does, so a `%23` in a name stays part of it and
/// what's opened is what was looked at.
fn url_path(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    percent_decode(&url[..end])
}

/// `%20` and the like. Bytes that don't make UTF-8 are replaced, never dropped.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(v) = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path as a `file://` URL: every byte but letters, digits and `/-_.~`
/// percent-encoded, so `#`, `?`, `%` and spaces stay part of the name. The
/// only way the app makes a file URL (`URL(fileURLWithPath:)`).
pub fn file_url(path: &str) -> String {
    // A backslash is a separator only on Windows. Elsewhere it's part of a
    // name, and turning it into `/` would make the URL name another file
    // than the one looked at: `Payload.app\` is a plain folder beside the
    // app `Payload.app` (Codex review, 10 Nov 2026).
    let mut path = if cfg!(windows) { path.replace('\\', "/") } else { path.to_owned() };
    if !path.starts_with('/') {
        // C:/Users/... is file:///C:/Users/...
        path.insert(0, '/');
    }
    let mut out = String::with_capacity(path.len() + 7);
    out.push_str("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            // The drive's colon, left as it is.
            b':' if cfg!(windows) && out.len() == 9 => out.push(':'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `path` made absolute: `~/` from home, a relative one from the thread's
/// folder. None when a relative path has no folder to be from.
pub fn absolute(path: &str, cwd: &str, home: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("~/") {
        return (!home.is_empty()).then(|| format!("{}/{rest}", home.trim_end_matches('/')));
    }
    if path.starts_with('/') || crate::reducer::text::is_windows_path(path) {
        return Some(path.to_owned());
    }
    if cwd.is_empty() {
        return None;
    }
    Some(format!("{}/{}", cwd.trim_end_matches(['/', '\\']), path.trim_start_matches("./")))
}

// ---------------------------------------------------------------- files

/// Extensions macOS opens by running something (`LinkPolicy.codeExtensions`),
/// and the other systems' programs and scripts. Never opened without asking,
/// whatever app is set for them.
pub fn runs_code(path: &str) -> bool {
    crate::highlights::runs_code(path)
}

/// Kinds of file that open to be looked at: pictures, films, sound, PDFs,
/// text and source code, office documents and fonts (`OpenGate.documentTypes`).
/// Scripts aren't here: Python Launcher and the like run them as they open.
/// Anything not here asks, as an app can claim any extension.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    // pictures
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "ico", "svg", "avif",
    // films and sound
    "mp4", "mov", "m4v", "webm", "mkv", "avi", "mp3", "wav", "m4a", "aac", "aiff", "aif", "flac", "ogg", "opus",
    // PDFs and text
    "pdf", "txt", "text", "md", "markdown", "mdx", "rtf", "csv", "tsv", "log", "json", "jsonc", "jsonl", "yaml", "yml", "toml", "ini", "xml", "plist", "html", "htm", "css",
    "scss", "less", "diff", "patch", "srt", "vtt",
    // source code that's compiled, not run as it opens
    "swift", "rs", "ts", "tsx", "jsx", "c", "h", "m", "mm", "cc", "cpp", "hpp", "cs", "java", "kt", "kts", "go", "sql", "graphql", "proto", "gradle",
    // office documents
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "numbers", "pages", "odt", "ods", "odp", "rtfd",
    // fonts
    "ttf", "otf", "woff", "woff2",
];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_document(path: &Path) -> bool {
    DOCUMENT_EXTENSIONS.contains(&extension(path).as_str())
}

/// Apps that run the file they're given (`LinkPolicy.codeRunnerApps`).
const CODE_RUNNER_APPS: &[&str] = &[
    "com.apple.terminal", "com.googlecode.iterm2", "org.python.pythonlauncher", "com.apple.installer", "com.apple.shortcuts", "com.apple.jarlauncher",
    "com.apple.automator.automator-application-stub", "com.apple.systempreferences", "com.apple.systemsettings", "com.apple.scripteditor.id.scriptapplet",
    "net.kovidgoyal.kitty", "dev.warp.warp-stable", "com.mitchellh.ghostty", "io.alacritty", "co.zeit.hyper", "com.github.wez.wezterm", "com.panic.prompt3",
];

pub fn is_code_runner(bundle_id: &str) -> bool {
    CODE_RUNNER_APPS.contains(&bundle_id.to_ascii_lowercase().as_str())
}

/// The app the system would open a file in, as far as this machine can say:
/// its bundle id and name on a Mac. The GPUI app asks Launch Services; the
/// tests hand in their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpeningApp {
    /// Where the app is: the file itself when it's an app.
    pub path: PathBuf,
    pub bundle_id: Option<String>,
    pub name: String,
}

/// Where `path` really is, every link followed: std's canonicalize, but on
/// Windows by its drive path (`C:\...`) rather than the verbatim form
/// (`\\?\C:\...`) std gives. `file_url` made that `file:////%3F/C%3A/...`,
/// and a question showed it as it was (PR #57). The verbatim form stays where
/// the drive path would name something else: a name Windows reserves, a path
/// over 260 characters (dunce's checks), and a name that's a device however
/// it ends (`is_device_name`). Everything that hands a resolved path on (to a
/// URL, a question, a drag, the pane's player) goes through here, so they all
/// name a file alike. On Mac and Linux it is std's canonicalize.
///
/// On Windows a share or a device (`\\server\share`, `\\?\UNC\...`,
/// `\\.\pipe\...`) is refused, before anything touches it: looking at
/// `\\host\share\a.md` would have Windows reach that machine, and offer it
/// your sign-in, from a link a reply wrote. That's what `file://host/a` is
/// refused for in `link_kind`. Mapped shares and links to them are refused
/// too: local_path walks links without following them into the network.
pub fn real_path(path: &Path) -> std::io::Result<PathBuf> {
    if on_a_share(path) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "a network share or a device"));
    }
    #[cfg(windows)]
    let local = crate::local_path::LocalPath::new(path)?;
    #[cfg(windows)]
    let real = local.path.clone();
    #[cfg(not(windows))]
    let real = std::fs::canonicalize(path)?;
    let simple = dunce::simplified(&real);
    // dunce reads only the last extension off a name, so `NUL.tar.gz` passes
    // its check, and it doesn't know COM0, COM¹ and the like. Stripped, those
    // name the device rather than the file canonicalize found (Codex's review
    // of PR #57, 12 Nov).
    let changed = simple.as_os_str().len() != real.as_os_str().len();
    if changed && simple.components().any(|c| matches!(c, std::path::Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Ok(real);
    }
    Ok(simple.to_path_buf())
}

/// Whether Windows reads a file name as a device: its part before the first
/// dot, less trailing spaces, is one of the names Microsoft lists ("Naming
/// Files, Paths, and Namespaces": `NUL.tar.gz` is `NUL`), or the console's.
pub fn is_device_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or_default().trim_end_matches(' ').to_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    ["COM", "LPT"].iter().any(|p| base.strip_prefix(p).is_some_and(|n| (n.len() == 1 && n.as_bytes()[0].is_ascii_digit()) || matches!(n, "¹" | "²" | "³")))
}

/// Whether Windows takes `path` to a network share, a device or a namespace
/// other than a drive's: anything starting with two slashes either way
/// (`\\host\share`, `//host/share`, `\\?\UNC\...`, `\\.\pipe\...`,
/// `\\?\GLOBALROOT\...`) or with `\??\`, which Windows passes on as an NT
/// path, except `\\?\C:\...`, a drive's verbatim form. Read from the text,
/// not std's prefix parsing, so it's the same test as Windows' own.
pub fn is_share_or_device(path: &str) -> bool {
    let s = path.as_bytes();
    let sep = |i: usize| s.get(i).is_some_and(|b| matches!(b, b'\\' | b'/'));
    if sep(0) && s.get(1..3) == Some(b"??") && sep(3) {
        return true;
    }
    if !(sep(0) && sep(1)) {
        return false;
    }
    let verbatim_drive = s.get(2) == Some(&b'?') && sep(3) && s.get(4).is_some_and(u8::is_ascii_alphabetic) && s.get(5) == Some(&b':') && (sep(6) || s.len() == 6);
    !verbatim_drive
}

/// On Windows, a path to a share or a device, or one with a part Windows
/// reads as a device (`C:\work\NUL.pdf`), which only looking at would open
/// (Codex's second look at PR #57). Never on Mac and Linux, where `//a/b` is
/// `/a/b` and `NUL` is a name like any other.
pub(crate) fn on_a_share(path: &Path) -> bool {
    let text = path.to_string_lossy();
    cfg!(windows) && (is_share_or_device(&text) || text.split(['\\', '/']).any(is_device_name))
}

/// QueryDosDevice may return a physical volume or a redirector with a path
/// tail. Only an exact physical-volume name is safe to open as a root.
pub fn is_local_volume_device(name: &str) -> bool {
    [r"\Device\HarddiskVolume", r"\Device\CdRom"].iter().any(|prefix| name.strip_prefix(prefix).is_some_and(|tail| !tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit())))
}

/// A checked local path as the Windows shell accepts it. Filesystem reads
/// keep their checked handle instead of looking this display path up again.
pub fn system_path(path: &Path) -> &Path { dunce::simplified(path) }

/// A reply's file probe uses the same local-path rules as opening it. On
/// Windows even metadata can contact a share, so never probe the raw path.
pub fn local_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    #[cfg(windows)]
    { crate::local_path::LocalPath::stat(path)?.file()?.metadata() }
    #[cfg(not(windows))]
    { std::fs::metadata(path) }
}

/// Keep the checked components held until an operation opens the resolved
/// path. Callers must use this path, not re-open the original spelling.
pub fn with_local_path<T>(path: &Path, read: impl FnOnce(&Path) -> std::io::Result<T>) -> std::io::Result<T> {
    #[cfg(windows)]
    {
        let local = crate::local_path::LocalPath::new(path)?;
        read(&local.anchored_name()?)
    }
    #[cfg(not(windows))]
    read(path)
}

/// A handle opened under the path guard. Reading this handle later never
/// follows a link swapped into the path in the meantime.
pub fn local_open(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    { crate::local_path::LocalPath::for_read(path)?.readable() }
    #[cfg(not(windows))]
    { open_without_waiting(&std::fs::canonicalize(path)?) }
}

/// Open the checked file object for a text edit, without another path lookup.
pub fn local_open_for_write(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    { crate::local_path::LocalPath::for_read(path)?.writable() }
    #[cfg(not(windows))]
    { std::fs::OpenOptions::new().write(true).open(path) }
}

/// Read a local file with its Windows ancestors held against a link swap.
pub fn local_read(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = local_open(path)?;
    if !file.metadata()?.is_file() { return Err(std::io::Error::other("Not a file")); }
    let mut bytes = Vec::new();
    file.take(max as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Open the directory while its checked Windows ancestors are held.
pub fn local_read_dir(path: &Path) -> std::io::Result<Vec<std::io::Result<LocalDirEntry>>> {
    #[cfg(windows)]
    { crate::local_path::LocalPath::for_read(path)?.entries() }
    #[cfg(not(windows))]
    {
        Ok(std::fs::read_dir(path)?.take(20_001).map(|e| {
            let e = e?;
            Ok(LocalDirEntry { path: e.path(), name: e.file_name(), is_folder: e.file_type()?.is_dir() })
        }).collect())
    }
}

pub struct LocalDirEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: std::ffi::OsString,
    pub is_folder: bool,
}

impl LocalDirEntry {
    pub fn path(&self) -> PathBuf { self.path.clone() }
    pub fn file_name(&self) -> std::ffi::OsString { self.name.clone() }
}

/// What looking at a file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    /// The real file, symlinks followed: what opens.
    pub resolved: PathBuf,
    /// Why opening it would or might run something, or None.
    pub runs: Option<String>,
    /// The path given was a symlink, or went through one.
    pub linked: bool,
}

/// Looks at the file at `path` (absolute) after following its symlinks.
/// None when nothing's there. `app_for` says which app would open the real
/// file (None where the system can't say).
pub fn look(path: &Path, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> Option<Look> {
    #[cfg(windows)]
    let _local = crate::local_path::LocalPath::new(path).ok()?;
    #[cfg(windows)]
    let resolved = system_path(&_local.path).to_path_buf();
    #[cfg(windows)]
    let meta = _local.file().ok()?.metadata().ok()?;
    #[cfg(not(windows))]
    let resolved = real_path(path).ok()?;
    #[cfg(not(windows))]
    let meta = local_metadata(path).ok()?;
    #[cfg(not(windows))]
    let linked = std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) || resolved.file_name() != path.file_name();
    #[cfg(windows)]
    let linked = _local.linked;
    let runs = runs(&resolved, &meta, app_for);
    Some(Look { resolved, runs, linked })
}

/// Why opening `file` would or might run something (`OpenGate.runs`).
fn runs(file: &Path, meta: &std::fs::Metadata, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> Option<String> {
    let name = file.to_string_lossy();
    if runs_code(&name) {
        return Some(format!("It's a .{} file, which runs when it's opened.", extension(file)));
    }
    if meta.is_dir() {
        // A bundle is a folder macOS launches or installs rather than shows.
        if ["Contents/Info.plist", "Contents/MacOS", "Info.plist"].iter().any(|p| local_metadata(&file.join(p)).is_ok()) {
            return Some("It's an app or a bundle, which runs when it's opened.".to_owned());
        }
    }
    let app = app_for(file);
    if let Some(app) = &app {
        // An app opens as itself: Launch Services names a bundle with no
        // known extension that way too.
        if app.path == file {
            return Some("It's a program.".to_owned());
        }
        if app.bundle_id.as_deref().is_some_and(is_code_runner) {
            return Some(format!("It opens in {}, which runs it.", app.name));
        }
    }
    // A plain folder goes to the file browser.
    if meta.is_dir() {
        return None;
    }
    if is_document(file) {
        return None;
    }
    if is_executable(meta) {
        return Some("It's marked as a program, so opening it may run it.".to_owned());
    }
    let opener = app.map(|a| format!(" It opens in {},", a.name)).unwrap_or_default();
    Some(format!("Threadbear can't tell what this kind of file does when it's opened.{opener} which may run what's in it."))
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// What to do with a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileVerdict {
    /// Open it: the URL is the real file's, built by `file_url`.
    Open { url: String, resolved: PathBuf },
    /// Ask first, then open `url` only on a yes (and only if the file is
    /// still what was asked about: `still_same`).
    Ask { question: Question, url: String, look: Look },
    /// Don't, and say why.
    Refuse(String),
}

/// What asks Mike: the question's words and its yes button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub detail: String,
    pub yes: String,
}

/// Decides about the file at `path`, which must be absolute already.
pub fn decide_file(path: &str, origin: Origin, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> FileVerdict {
    let name = crate::highlights::last_component(path).to_owned();
    if origin == Origin::OtherMac {
        return FileVerdict::Refuse(format!("{} is on another Mac, so Threadbear doesn't open it here. It shows in the pane instead.", if name.is_empty() { "That file" } else { &name }));
    }
    let given = Path::new(path);
    if on_a_share(given) {
        return FileVerdict::Refuse(format!("{} is on a network share or names a device, so Threadbear doesn't open it from here.", if name.is_empty() { "That file" } else { &name }));
    }
    if !given.is_absolute() {
        return FileVerdict::Refuse(format!("Threadbear couldn't tell where {name} is."));
    }
    let Some(look) = look(given, app_for) else {
        return FileVerdict::Refuse(format!("{} isn't there any more.", if name.is_empty() { "That file" } else { &name }));
    };
    // Still in the verbatim form, `real_path` found no drive path that names
    // the same file, and `file_url` would make `file:////%3F/...` of it,
    // which opens nothing (Codex's second look at PR #57).
    if cfg!(windows) && look.resolved.to_string_lossy().starts_with(r"\\") {
        let real = look.resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return FileVerdict::Refuse(format!("Threadbear can't open {real} on Windows yet: its path is too long, or it's on a network drive."));
    }
    let url = file_url(&look.resolved.to_string_lossy());
    match &look.runs {
        None => FileVerdict::Open { url, resolved: look.resolved },
        Some(why) => {
            let real = look.resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let link = if look.linked { format!("\n\n{name} is a link to {real}.") } else { String::new() };
            let question = Question {
                title: format!("Open {real}?"),
                detail: format!("{why} It's from a thread, so Threadbear asks before running it.{link}\n\n{}", look.resolved.to_string_lossy()),
                yes: "Open".to_owned(),
            };
            FileVerdict::Ask { question, url, look }
        }
    }
}

/// After a yes: the file at `path` still resolves to what was asked about,
/// and is still the same kind, so a swap while the question was up doesn't
/// open something else.
pub fn still_same(path: &str, asked: &Look, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> bool {
    look(Path::new(path), app_for).is_some_and(|now| now.resolved == asked.resolved && now.runs == asked.runs)
}

// ---------------------------------------------------------------- shown in the app

/// What the app shows in a view of its own rather than handing to another
/// app: a video in AVKit's player, a PDF in PDFKit's view (on a Mac, where
/// those views are; elsewhere they still open in their own app).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Video,
    Pdf,
}

/// Films AVFoundation plays on every Mac this runs on: MPEG-4 and QuickTime
/// movies. WebM, MKV and AVI aren't among them, so they open in their app.
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov"];

/// What a path's name says it is, for choosing the view. `decide_media`
/// checks the real file before anything reads it.
pub fn media_kind(path: &str) -> Option<Media> {
    let ext = extension(Path::new(path));
    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        Some(Media::Video)
    } else if ext == "pdf" {
        Some(Media::Pdf)
    } else {
        None
    }
}

/// The brands an MPEG-4 or QuickTime film's `ftyp` box leads with. Pictures
/// use the same box shape (HEIF's `heic` and `mif1`, AVIF's `avif`), so the
/// box alone doesn't say it's a film: its main brand does.
const VIDEO_BRANDS: &[&[u8; 4]] = &[
    b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"iso7", b"iso8", b"iso9", b"mp41", b"mp42", b"mp71", b"avc1", b"M4V ", b"M4VH", b"M4VP", b"qt  ", b"3gp4", b"3gp5", b"3gp6",
    b"3g2a", b"MSNV", b"dash", b"XAVC",
];

/// The brands of still pictures in the same box shape (HEIF, AVIF and their
/// picture sequences): an `ftyp` naming one of these anywhere in its list is
/// a picture's, whatever its main brand says.
const PICTURE_BRANDS: &[&[u8; 4]] = &[b"mif1", b"mif2", b"msf1", b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"heif", b"avif", b"avis", b"avci", b"avcs", b"jpeg", b"jpgs"];

/// The boxes an older QuickTime movie can have before its `moov` (or a newer
/// file before its `ftyp`): padding, the film's own data, a preview.
const QUICKTIME_LEADING: &[&[u8; 4]] = &[b"free", b"skip", b"wide", b"mdat", b"pnot"];

/// How many of those boxes are walked past before giving up on finding an
/// `ftyp` or a `moov`.
const LEADING_BOXES: usize = 8;

/// How many boxes in all are walked: after a `moov`, every box to the end of
/// the file is looked at for a picture's `ftyp`, and a file with more top
/// level boxes than this (a real movie without an `ftyp` has a handful) isn't
/// taken for one (Codex's fourth review, 12 Nov).
const ALL_BOXES: usize = 64;

/// Whether a file of `len` bytes, whose first bytes are `head`, starts the
/// way its kind's files do (`looks_like_at`, reading only `head`).
pub fn looks_like(kind: Media, head: &[u8], len: u64) -> bool {
    looks_like_at(
        kind,
        &mut |at, buf| {
            let start = usize::try_from(at).unwrap_or(usize::MAX).min(head.len());
            let n = buf.len().min(head.len() - start);
            buf[..n].copy_from_slice(&head[start..start + n]);
            n
        },
        len,
    )
}

/// Whether a file of `len` bytes starts the way its kind's files do,
/// reading it through `read_at` (bytes at an offset, how many it read).
///
/// An MPEG-4 or QuickTime file is a run of boxes, each a 4-byte size and a
/// 4-letter type (a size of 1 is "a 64-bit size follows", 0 "to the end of
/// the file"). A film's first box is `ftyp`, a few dozen bytes long, whose
/// brands are a film's; an older QuickTime movie may have padding (`free`,
/// `skip`, `wide`), its data (`mdat`) or a preview (`pnot`) before its
/// `moov` or `ftyp`. Those are walked past, a few at most, each with a sane
/// size that fits in the file, until an `ftyp` or a `moov`; anything else
/// first, or a box that doesn't fit, isn't a film. An `ftyp` naming a
/// picture's brand is refused wherever it is (Codex's second review, 12 Nov:
/// eight bytes of `free` in front of an AVIF named `.mov` passed when only
/// the first box was looked at). A PDF starts `%PDF-` and its version.
pub fn looks_like_at(kind: Media, read_at: &mut dyn FnMut(u64, &mut [u8]) -> usize, len: u64) -> bool {
    match kind {
        Media::Video => film_boxes(read_at, len),
        Media::Pdf => {
            let mut head = [0u8; 6];
            let n = read_at(0, &mut head);
            n == 6 && head.starts_with(b"%PDF-") && head[5].is_ascii_digit()
        }
    }
}

fn film_boxes(read_at: &mut dyn FnMut(u64, &mut [u8]) -> usize, len: u64) -> bool {
    let mut at = 0u64;
    // A `moov` found before any `ftyp`: an old QuickTime movie, unless an
    // `ftyp` after it names a picture (Codex's third review, 12 Nov: an
    // empty `moov` in front of an AVIF passed).
    let mut movie = false;
    for walked in 0..ALL_BOXES {
        if !movie && walked >= LEADING_BOXES {
            return false;
        }
        let mut header = [0u8; 16];
        let got = read_at(at, &mut header);
        if got < 8 {
            return movie;
        }
        let kind: [u8; 4] = [header[4], header[5], header[6], header[7]];
        let small = u64::from(u32::from_be_bytes([header[0], header[1], header[2], header[3]]));
        let rest = len - at;
        let size = match small {
            // To the end of the file: only the movie itself can be last
            // without anything after it to find.
            0 => rest,
            1 => {
                if got < 16 {
                    return false;
                }
                let big = u64::from_be_bytes([header[8], header[9], header[10], header[11], header[12], header[13], header[14], header[15]]);
                // A 64-bit size counts its own 16-byte header.
                if big < 16 {
                    return false;
                }
                big
            }
            n if n < 8 => return false,
            n => n,
        };
        if size > rest {
            return false;
        }
        if &kind == b"ftyp" {
            return small != 0 && small != 1 && ftyp_is_a_films(read_at, at, size);
        }
        if &kind == b"moov" {
            // One with nothing in it isn't a movie.
            let header_len = if small == 1 { 16 } else { 8 };
            if size <= header_len {
                return false;
            }
            movie = true;
        } else if !movie && !QUICKTIME_LEADING.contains(&&kind) {
            // Before the movie, only what QuickTime puts there; after it,
            // anything (its user data, say), walked past to the end.
            return false;
        }
        if small == 0 {
            return movie;
        }
        at = match at.checked_add(size) {
            Some(next) if next < len => next,
            _ => return movie,
        };
    }
    false
}

/// An `ftyp` box at `at`, `size` bytes: a few dozen bytes, whole brands, a
/// film's main brand, and no picture's brand among the rest.
fn ftyp_is_a_films(read_at: &mut dyn FnMut(u64, &mut [u8]) -> usize, at: u64, size: u64) -> bool {
    if !(16..=4096).contains(&size) || size % 4 != 0 {
        return false;
    }
    let mut body = vec![0u8; size as usize];
    if read_at(at, &mut body) != body.len() {
        return false;
    }
    let brand = |i: usize| -> &[u8] { &body[i..i + 4] };
    if !VIDEO_BRANDS.iter().any(|b| brand(8) == *b) {
        return false;
    }
    // The minor version at 12, then the compatible brands from 16.
    (16..body.len()).step_by(4).all(|i| !PICTURE_BRANDS.iter().any(|b| brand(i) == *b))
}

/// A file the rules have checked, held open: the view reads this, not the
/// path again, so a file swapped in at the path afterwards (or a folder
/// above it swapped for a link) is never what's shown.
#[derive(Debug)]
pub struct CheckedFile {
    /// Where it was, symlinks followed: for its name, its folder (a PDF's
    /// relative links) and checking a drag still means this file.
    pub real: PathBuf,
    pub file: std::fs::File,
    pub len: u64,
}

/// What to do with a video or PDF the pane or the review window would show.
#[derive(Debug)]
pub enum MediaVerdict {
    /// Show this file, the one held open.
    Show(CheckedFile),
    /// Don't, and say why.
    Refuse(String),
}

/// Decides about showing the file at `path` (absolute) in the app's own
/// view: only this Mac's own file (another Mac's path names a file on that
/// Mac, and one here at the same path isn't it), and only a real one of the
/// kind its name says, after its symlinks are followed, so a `.mp4` that's a
/// link to a key file or a script is never read as a film. The real file is
/// opened once, without following a link put there since and without waiting
/// on a pipe, and everything after that is checked on what was opened.
pub fn decide_media(path: &str, origin: Origin) -> MediaVerdict {
    let given = crate::highlights::last_component(path);
    let name = if given.is_empty() { "That file" } else { given };
    let Some(kind) = media_kind(path) else {
        return MediaVerdict::Refuse(format!("{name} isn't a video or a PDF, so it opens in its own app."));
    };
    let (what, verb) = match kind {
        Media::Video => ("a video", "play it"),
        Media::Pdf => ("a PDF", "show it"),
    };
    if origin == Origin::OtherMac {
        return MediaVerdict::Refuse(format!("{name} is on another Mac, so it can't {} here yet.", if kind == Media::Video { "play" } else { "show" }));
    }
    let given_path = Path::new(path);
    if on_a_share(given_path) {
        return MediaVerdict::Refuse(format!("{name} is on a network share or names a device, so Threadbear doesn't open it from here."));
    }
    if !given_path.is_absolute() {
        return MediaVerdict::Refuse(format!("Threadbear couldn't tell where {name} is."));
    }
    #[cfg(windows)]
    let _local = match crate::local_path::LocalPath::new(given_path) {
        Ok(local) => local,
        Err(_) => return MediaVerdict::Refuse(format!("{name} isn't there any more, or isn't an accessible local file.")),
    };
    let Ok(resolved) = real_path(given_path) else {
        return MediaVerdict::Refuse(format!("{name} isn't there any more."));
    };
    let not_really = || MediaVerdict::Refuse(format!("{name} isn't really {what}, so Threadbear didn't {verb}."));
    if media_kind(&resolved.to_string_lossy()) != Some(kind) {
        return not_really();
    }
    #[cfg(windows)]
    let opened = local_open(given_path);
    #[cfg(not(windows))]
    let opened = open_without_waiting(&resolved);
    let Ok(mut file) = opened else {
        return MediaVerdict::Refuse(format!("{name} changed or went while Threadbear looked at it, so it didn't {verb}."));
    };
    let len = match file.metadata() {
        Ok(meta) if meta.is_file() => meta.len(),
        _ => return not_really(),
    };
    if !looks_like_at(kind, &mut |at, buf| read_at(&file, at, buf), len) {
        return not_really();
    }
    {
        // From the start for the view (Windows's positioned reads move it).
        use std::io::Seek;
        let _ = file.seek(std::io::SeekFrom::Start(0));
    }
    MediaVerdict::Show(CheckedFile { real: resolved, file, len })
}

/// Reads what it can of `buf` from `file` at `at`, without moving where the
/// file is read from next (the view reads it from its start).
fn read_at(file: &std::fs::File, at: u64, buf: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buf.len() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, &mut buf[got..], at + got as u64);
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[got..], at + got as u64);
        match n {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    got
}

/// Opens the real file for reading: not through a link swapped in for it
/// since it was resolved (O_NOFOLLOW), and without waiting if it's become a
/// pipe, which would hold the app until something wrote to it (O_NONBLOCK;
/// the opened file's own type is checked after).
fn open_without_waiting(real: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    options.open(real)
}

/// A link shown in a question: long ones cut in the middle (`LinkPolicy.shown`).
pub fn shown(url: &str, limit: usize) -> String {
    let count = url.chars().count();
    if count <= limit {
        return url.to_owned();
    }
    let head: String = url.chars().take(limit * 2 / 3).collect();
    let tail: String = url.chars().skip(count - limit / 3).collect();
    format!("{head}…{tail}")
}

/// The question for a link that starts another app (`OpenGate.openOther`).
pub fn link_question(url: &str, app: &str) -> Question {
    Question {
        title: format!("Open this link in {app}?"),
        detail: format!("It isn't a web page: it starts {app}, which acts on what the link says. It's from a thread, so Threadbear asks first.\n\n{}", shown(url, 300)),
        yes: format!("Open in {app}"),
    }
}

/// Threadbear's own scheme: only ever a thread, never handed back to the system.
pub fn is_own_scheme(url: &str) -> bool {
    scheme(url).is_some_and(|s| s == "threadbear")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn none(_: &Path) -> Option<OpeningApp> {
        None
    }

    /// A folder of its own under the system's temp folder, removed at the end.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("tb-open-policy-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            // As `look` resolves paths, so a path made here is the one it gives back.
            Scratch(dunce::canonicalize(&dir).unwrap())
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
        fn file(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.path(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
            p
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app_bundle(s: &Scratch, rel: &str) -> PathBuf {
        let app = s.path(rel);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
        app
    }

    /// Finding 58: a skill path with a `#` in it named the sibling `.app`
    /// once pasted into `file://`. The URL is built from the path, and the
    /// path is what's looked at, so the `.app` never comes into it.
    #[test]
    fn a_hash_in_a_path_stays_part_of_the_name() {
        let s = Scratch::new("hash");
        app_bundle(&s, ".agents/skills/Inspect.app");
        let skill = s.file(".agents/skills/Inspect.app#help/SKILL.md", "# skill");
        let url = file_url(&skill.to_string_lossy());
        assert!(url.ends_with("/Inspect.app%23help/SKILL.md"), "{url}");
        assert!(!url.contains('#'));
        match decide_file(&skill.to_string_lossy(), Origin::ThisMac, &none) {
            FileVerdict::Open { url: opened, resolved } => {
                assert_eq!(resolved, skill);
                assert_eq!(opened, url);
            }
            other => panic!("{other:?}"),
        }
        // A link to it written as a URL keeps the %23 in the name, and a
        // bare # cuts the fragment off, as a URL parser would, before the
        // path is decoded: the .app is a folder, so it's refused as a program.
        #[cfg(not(windows))]
        {
            let encoded = format!("{}/.agents/skills/Inspect.app%23help/SKILL.md", s.0.to_string_lossy());
            assert_eq!(link_kind(&encoded), LinkKind::File(skill.to_string_lossy().into_owned()));
            let bare = ["file://", &s.0.to_string_lossy(), "/.agents/skills/Inspect.app#help/SKILL.md"].concat();
            let LinkKind::File(cut) = link_kind(&bare) else { panic!() };
            assert!(cut.ends_with("/Inspect.app"));
            assert!(matches!(decide_file(&cut, Origin::ThisMac, &none), FileVerdict::Ask { .. }));
        }
        // The two above are Unix paths: `file://` plus a drive path isn't a
        // URL. A Windows path with a drive letter is taken as written, # and
        // all, so nothing in it is cut off.
        #[cfg(windows)]
        assert_eq!(link_kind(&skill.to_string_lossy()), LinkKind::File(skill.to_string_lossy().into_owned()));
    }

    /// Findings 59 and 64: a `.command` (or any program) a reply links to,
    /// inline code or a Markdown link, asks first.
    #[test]
    fn programs_ask_first() {
        let s = Scratch::new("programs");
        for rel in ["docs/report.command", "run.sh", "x.tool", "Setup.pkg", "a.terminal", "go.webloc", "a.jar"] {
            let p = s.file(rel, "#!/bin/sh\necho hi\n");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }), "{rel}");
        }
        let app = app_bundle(&s, "Tool.app");
        assert!(matches!(decide_file(&app.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }));
        // A bundle without the name: still a bundle.
        let plain = app_bundle(&s, "Notes");
        assert!(matches!(decide_file(&plain.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }));
    }

    #[test]
    fn scripts_and_unknown_kinds_ask() {
        let s = Scratch::new("scripts");
        for rel in ["build.py", "a.js", "tool.rb", "x.applescript", "noextension", "thing.weird"] {
            let p = s.file(rel, "print(1)");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }), "{rel}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_without_a_kind_asks() {
        use std::os::unix::fs::PermissionsExt;
        let s = Scratch::new("exec");
        let p = s.file("payload", "#!/bin/sh\n");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let FileVerdict::Ask { question, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(question.detail.contains("marked as a program"));
    }

    #[test]
    fn documents_and_folders_open() {
        let s = Scratch::new("docs");
        for rel in ["docs/report.md", "shot.png", "a.pdf", "src/main.rs", "data.csv", "page.html"] {
            let p = s.file(rel, "x");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { .. }), "{rel}");
        }
        assert!(matches!(decide_file(&s.path("docs").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { .. }));
    }

    /// `std::fs::canonicalize` gives a Windows path in its verbatim form,
    /// `\\?\C:\...`. Built into a URL that was `file:////%3F/C%3A/...`, and a
    /// question named the file by it. The real file opens by its drive path.
    #[cfg(windows)]
    #[test]
    fn a_file_on_windows_opens_by_its_drive_path() {
        let s = Scratch::new("drive");
        let doc = s.file("notes.md", "x");
        let FileVerdict::Open { url, resolved } = decide_file(&doc.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        let text = resolved.to_string_lossy().into_owned();
        assert!(!text.starts_with(r"\\"), "{text}");
        assert!(url.starts_with(&["file:///", &text[..1], ":/"].concat()), "{url}");
        assert!(url.ends_with("/notes.md"), "{url}");
        let program = s.file("go.command", "x");
        let FileVerdict::Ask { question, .. } = decide_file(&program.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(!question.detail.contains(r"\\?\"), "{}", question.detail);
    }

    /// Names Windows reads as a device however they end, which dunce lets
    /// through as a drive path (Codex's review of PR #57).
    #[test]
    fn device_names_are_known_whatever_follows_them() {
        for name in ["NUL", "nul.txt", "NUL.tar.gz", "Con .md", "com0", "COM9.x", "lpt¹.md", "LPT³", "CONIN$", "conout$.log"] {
            assert!(is_device_name(name), "{name}");
        }
        for name in ["NULL", "console.md", "COM10", "LPT", "notes.md", "AUXILIARY.txt", "xNUL.md", "COMx"] {
            assert!(!is_device_name(name), "{name}");
        }
    }

    /// What Windows would take to another machine or a device, read from
    /// the text alike everywhere.
    #[test]
    fn shares_and_devices_are_known_by_their_start() {
        for path in [r"\\host\share\a.md", "//host/share/a.md", r"\/host/share", r"\\?\UNC\host\share\a.md", r"\\.\pipe\x", r"\\?\GLOBALROOT\Device\x", r"\??\UNC\host\share\x", r"\??\C:\x", r"\\?\Volume{1}\x"] {
            assert!(is_share_or_device(path), "{path}");
        }
        for path in [r"C:\work\a.md", "C:/work/a.md", r"\\?\C:\work\a.md", r"\\?\c:", r"\work\a.md", "/Users/m/a.md", "a.md", ""] {
            assert!(!is_share_or_device(path), "{path}");
        }
    }

    /// A share or a device a reply links to is refused before anything looks
    /// at it: looking would reach that machine with the user's sign-in.
    #[cfg(windows)]
    #[test]
    fn a_share_or_a_device_on_windows_is_refused_unlooked() {
        for path in [r"\\attacker\share\a.md", "//attacker/share/a.md", r"\\?\UNC\attacker\share\a.md", r"\\.\pipe\x", r"\\?\GLOBALROOT\Device\x.md"] {
            assert!(matches!(decide_file(path, Origin::ThisMac, &none), FileVerdict::Refuse(why) if why.contains("network share")), "{path}");
            assert!(matches!(decide_media(&format!("{path}.mp4"), Origin::ThisMac), MediaVerdict::Refuse(why) if why.contains("network share")), "{path}");
            assert!(real_path(Path::new(path)).is_err(), "{path}");
        }
        // A part named as a device, however it ends, is refused unlooked too.
        let device = Path::new(r"C:\work\NUL.part.md");
        assert!(matches!(decide_file(&device.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Refuse(why) if why.contains("names a device")));
        // A drive's verbatim form is still this machine's file.
        let s = Scratch::new("verbatim");
        let doc = s.file("notes.md", "x");
        let verbatim = std::fs::canonicalize(&doc).unwrap();
        assert!(verbatim.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(real_path(&verbatim).unwrap(), doc);
    }

    #[cfg(windows)]
    #[test]
    fn windows_links_are_resolved_locally_before_any_file_probe() {
        use std::os::windows::fs::{symlink_file, symlink_dir};
        let s = Scratch::new("local-links");
        let doc = s.file("real/notes.md", "notes");
        let local = s.path("local.md");
        symlink_file(&doc, &local).unwrap();
        assert_eq!(real_path(&local).unwrap(), doc);
        assert_eq!(local_read(&local, 4096).unwrap(), b"notes");
        fs::create_dir_all(s.path("nested")).unwrap();
        let relative = s.path("nested/link.md");
        symlink_file(r"..\real\notes.md", &relative).unwrap();
        assert_eq!(local_read(&relative, 4096).unwrap(), b"notes");
        let dir_link = s.path("folder");
        symlink_dir(doc.parent().unwrap(), &dir_link).unwrap();
        assert_eq!(real_path(&dir_link.join("notes.md")).unwrap(), doc);
        // No server is needed: permission denied comes from the path guard,
        // not a failed network lookup. Both a final link and an ancestor.
        let remote = s.path("remote.md");
        symlink_file(r"\\127.0.0.1\tb-no-share\notes.md", &remote).unwrap();
        let ancestor = s.path("remote-folder");
        symlink_dir(r"\\127.0.0.1\tb-no-share", &ancestor).unwrap();
        let device = s.path("device.md");
        symlink_file(r"\\.\NUL", &device).unwrap();
        for path in [remote, ancestor.join("notes.md"), device] {
            assert_eq!(real_path(&path).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied, "{}", path.display());
            assert_eq!(local_metadata(&path).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(local_read(&path, 4096).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
            assert!(matches!(decide_file(&path.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Refuse(_)));
            assert!(crate::file_identity::FileIdentity::of(&path).is_none());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_urls_round_trip_to_a_drive_path() {
        assert_eq!(link_kind("file:///C:/work/notes%20one.md"), LinkKind::File("C:/work/notes one.md".into()));
        assert_eq!(link_kind("file://localhost/C:/work/notes.md"), LinkKind::File("C:/work/notes.md".into()));
        assert_eq!(link_kind("C:NUL"), LinkKind::File("C:NUL".into()));
        assert!(matches!(decide_file("C:NUL", Origin::ThisMac, &none), FileVerdict::Refuse(_)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_normalizes_dot_segments_before_following_links_and_refuses_ambiguous_names() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let s = Scratch::new("normalize");
        s.file("work/notes.md", "local");
        let dotted = format!(r"\\?\{}\work ", s.0.display());
        symlink_dir(r"\\127.0.0.1\tb-no-share", &dotted).unwrap();
        assert_eq!(local_metadata(&s.path(r"work \notes.md")).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        let elsewhere = s.file("elsewhere/x.md", "different file");
        fs::create_dir_all(s.path("elsewhere/sub")).unwrap();
        symlink_dir(s.path("elsewhere/sub"), s.path("dirlink")).unwrap();
        symlink_file(r"\\127.0.0.1\tb-no-share\x.md", s.path("x.md")).unwrap();
        assert_eq!(local_metadata(&s.path(r"dirlink\..\x.md")).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(local_read(&elsewhere, 64).unwrap(), b"different file");
        let listed = crate::local_files::list(&serde_json::json!({ "path": s.0 })).unwrap();
        assert!(listed["entries"].as_array().unwrap().iter().any(|e| e["name"] == "x.md" && e["size"].is_null()));
        assert!(crate::local_files::read(&serde_json::json!({ "path": s.path(r"dirlink\..\x.md") })).is_err());
        fs::remove_dir(&dotted).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_short_alias_cannot_hide_a_reserved_long_name() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        #[link(name = "kernel32")]
        extern "system" { fn GetShortPathNameW(long: *const u16, short: *mut u16, size: u32) -> u32; }
        let s = Scratch::new("reserved-alias");
        let file = PathBuf::from(format!(r"\\?\{}\NUL.part.md", s.0.display()));
        fs::write(&file, b"ordinary disk bytes").unwrap();
        let long: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut out = vec![0u16; 32768];
        let len = unsafe { GetShortPathNameW(long.as_ptr(), out.as_mut_ptr(), out.len() as u32) } as usize;
        assert!(len > 0 && len < out.len());
        // Volumes with 8.3 creation off return the long spelling unchanged;
        // both it and an alias on a volume with 8.3 enabled must be refused.
        let alias = PathBuf::from(std::ffi::OsString::from_wide(&out[..len]));
        assert_eq!(local_metadata(&alias).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert!(look(&alias, &none).is_none());
        assert!(crate::file_identity::FileIdentity::of(&alias).is_none());
        fs::remove_file(file).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_guards_hold_ancestors_and_read_handles_survive_a_swap() {
        use std::io::Read;
        let s = Scratch::new("locked");
        let doc = s.file("folder/notes.md", "the file opened");
        let guard = crate::local_path::LocalPath::new(&doc).unwrap();
        assert!(fs::rename(&doc, s.path("folder/old.md")).is_err(), "the checked file's name must be locked");
        drop(guard);
        let mut file = local_open(&doc).unwrap();
        fs::rename(&doc, s.path("folder/old.md")).unwrap();
        std::os::windows::fs::symlink_file(r"\\127.0.0.1\tb-no-share\notes.md", &doc).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"the file opened");
        drop(file);
        assert!(local_read(&doc, 64).is_err());
    }

    #[test]
    fn physical_volume_mappings_have_no_path_tail_or_redirector() {
        for name in [r"\Device\HarddiskVolume3", r"\Device\CdRom0"] { assert!(is_local_volume_device(name)); }
        for name in [r"\Device\HarddiskVolume3\repo\link", r"\Device\HarddiskVolumeShadowCopy1", r"\Device\Mup\server", r"\Device\CdRom0\x"] { assert!(!is_local_volume_device(name)); }
    }

    #[cfg(windows)]
    #[test]
    fn windows_editing_refuses_a_link_swap_and_reads_a_log_with_a_writer_open() {
        let s = Scratch::new("editor");
        let doc = s.file("notes.md", "original");
        let writer = fs::OpenOptions::new().write(true).open(&doc).unwrap();
        assert_eq!(local_read(&doc, 64).unwrap(), b"original");
        drop(writer);
        let mut editor = crate::text_file::TextFileDocument::new(&doc);
        assert_eq!(editor.load().unwrap(), "original");
        editor.save("local edit").unwrap();
        assert_eq!(fs::read(&doc).unwrap(), b"local edit");
        fs::rename(&doc, s.path("old.md")).unwrap();
        std::os::windows::fs::symlink_file(r"\\127.0.0.1\tb-no-share\notes.md", &doc).unwrap();
        assert!(editor.changed_on_disk().is_err());
        assert!(editor.save("do not send this").is_err());
        assert_eq!(fs::read(s.path("old.md")).unwrap(), b"local edit");
    }

    /// A file whose path is too long for its drive form keeps the verbatim
    /// one, which no URL the system opens can carry: refused, not handed on
    /// as `file:////%3F/...`.
    #[cfg(windows)]
    #[test]
    fn a_file_past_windows_path_limit_is_refused_not_opened_broken() {
        let s = Scratch::new("long");
        let deep = s.path(&["a".repeat(100), "b".repeat(100), "c".repeat(100)].join("\\"));
        // Made through the verbatim form, which has no length limit.
        fs::create_dir_all(format!(r"\\?\{}", deep.display())).unwrap();
        let doc = deep.join("notes.md");
        fs::write(format!(r"\\?\{}", doc.display()), "x").unwrap();
        assert!(real_path(&doc).unwrap().to_string_lossy().starts_with(r"\\?\"));
        assert!(matches!(decide_file(&doc.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Refuse(why) if why.contains("too long")));
    }

    /// A document whose app would run it (a `.md` set to open in Terminal).
    #[test]
    fn a_document_going_to_a_code_runner_asks() {
        let s = Scratch::new("runner");
        let p = s.file("notes.md", "x");
        let terminal = |_: &Path| Some(OpeningApp { path: PathBuf::from("/System/Applications/Utilities/Terminal.app"), bundle_id: Some("com.apple.Terminal".into()), name: "Terminal".into() });
        let FileVerdict::Ask { question, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &terminal) else { panic!() };
        assert!(question.detail.contains("Terminal, which runs it"));
        // An app the system says opens as itself.
        let me = s.file("Thing", "x");
        let itself = |p: &Path| Some(OpeningApp { path: p.to_owned(), bundle_id: Some("com.evil.thing".into()), name: "Thing".into() });
        assert!(matches!(decide_file(&me.to_string_lossy(), Origin::ThisMac, &itself), FileVerdict::Ask { .. }));
    }

    /// A symlink is judged, and opened, as the file it points at.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed() {
        let s = Scratch::new("links");
        let app = app_bundle(&s, "Payload.app");
        std::os::unix::fs::symlink(&app, s.path("README.md")).unwrap();
        let FileVerdict::Ask { question, url, look } = decide_file(&s.path("README.md").to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(look.linked);
        assert_eq!(look.resolved, app);
        assert!(url.ends_with("/Payload.app"));
        assert!(question.detail.contains("README.md is a link to Payload.app"));
        // The other way round, a program's name on a document opens the document.
        let doc = s.file("real.md", "x");
        std::os::unix::fs::symlink(&doc, s.path("fake.command")).unwrap();
        assert!(matches!(decide_file(&s.path("fake.command").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { resolved, .. } if resolved == doc));
    }

    /// A swap while the question's up isn't what was said yes to.
    #[cfg(unix)]
    #[test]
    fn a_swapped_file_isnt_opened_after_a_yes() {
        let s = Scratch::new("swap");
        let p = s.file("go.command", "#!/bin/sh\n");
        let FileVerdict::Ask { look, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(still_same(&p.to_string_lossy(), &look, &none));
        fs::remove_file(&p).unwrap();
        let other = app_bundle(&s, "Other.app");
        std::os::unix::fs::symlink(&other, &p).unwrap();
        assert!(!still_same(&p.to_string_lossy(), &look, &none));
    }

    #[test]
    fn another_macs_files_and_missing_ones_are_refused() {
        let s = Scratch::new("refuse");
        let p = s.file("a.md", "x");
        assert!(matches!(decide_file(&p.to_string_lossy(), Origin::OtherMac, &none), FileVerdict::Refuse(_)));
        assert!(matches!(decide_file(&s.path("gone.md").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Refuse(_)));
        assert!(matches!(decide_file("relative/a.md", Origin::ThisMac, &none), FileVerdict::Refuse(_)));
    }

    #[test]
    fn links_by_kind() {
        assert_eq!(link_kind("https://example.com/a?b#c"), LinkKind::Web("https://example.com/a?b#c".into()));
        assert_eq!(link_kind("HTTPS://example.com"), LinkKind::Web("HTTPS://example.com".into()));
        assert_eq!(link_kind("https://"), LinkKind::Refuse);
        assert_eq!(link_kind("https://a b.com"), LinkKind::Refuse);
        assert_eq!(link_kind("mailto:a@b.c"), LinkKind::Ask("mailto:a@b.c".into()));
        assert_eq!(link_kind("x-apple.systempreferences:com.apple.preference"), LinkKind::Ask("x-apple.systempreferences:com.apple.preference".into()));
        assert_eq!(link_kind("file:///tmp/a%20b.md#x"), LinkKind::File("/tmp/a b.md".into()));
        assert_eq!(link_kind("file://localhost/tmp/a.md"), LinkKind::File("/tmp/a.md".into()));
        assert_eq!(link_kind("file://evil.example/share/a.app"), LinkKind::Refuse);
        assert_eq!(link_kind("docs/report.command"), LinkKind::File("docs/report.command".into()));
        assert_eq!(link_kind("guide.md#links"), LinkKind::File("guide.md".into()));
        assert_eq!(link_kind("a\u{0}b"), LinkKind::Refuse);
        assert_eq!(link_kind("#usage"), LinkKind::Refuse);
        assert!(is_own_scheme("threadbear://thread/x"));
    }

    #[test]
    fn file_urls_escape_what_a_url_would_read() {
        assert_eq!(file_url("/a b/c#d?e%f.md"), "file:///a%20b/c%23d%3Fe%25f.md");
        if cfg!(windows) {
            assert_eq!(file_url("C:\\Users\\a.md"), "file:///C:/Users/a.md");
        } else {
            assert_eq!(file_url("/w/Payload.app\\"), "file:///w/Payload.app%5C");
        }
    }

    /// A folder named `Payload.app\` beside the app is judged and opened as
    /// itself, never as the app.
    #[cfg(unix)]
    #[test]
    fn a_backslash_in_a_name_stays_part_of_it() {
        let s = Scratch::new("backslash");
        app_bundle(&s, "Payload.app");
        let plain = s.path("Payload.app\\");
        fs::create_dir_all(&plain).unwrap();
        let LinkKind::File(path) = link_kind(&[&s.0.to_string_lossy(), "/Payload.app%5C"].concat()) else { panic!() };
        match decide_file(&path, Origin::ThisMac, &none) {
            FileVerdict::Open { url, resolved } => {
                assert_eq!(resolved, plain);
                assert!(url.ends_with("/Payload.app%5C"), "{url}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The start of a real MPEG-4 file: its `ftyp` box.
    const MP4_HEAD: &[u8] = b"\0\0\0\x20ftypisom\0\0\x02\0isomiso2avc1mp41";

    fn bytes(s: &Scratch, rel: &str, body: &[u8]) -> PathBuf {
        let p = s.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        p
    }

    /// The real file a verdict would show, or None for a refusal.
    fn shows(verdict: MediaVerdict) -> Option<PathBuf> {
        match verdict {
            MediaVerdict::Show(checked) => Some(checked.real),
            MediaVerdict::Refuse(_) => None,
        }
    }

    fn refusal(verdict: MediaVerdict) -> String {
        match verdict {
            MediaVerdict::Refuse(why) => why,
            MediaVerdict::Show(checked) => panic!("showed {:?}", checked.real),
        }
    }

    #[test]
    fn a_real_video_on_this_mac_plays() {
        let s = Scratch::new("media-real");
        let clip = bytes(&s, "clips/hero.mp4", MP4_HEAD);
        assert_eq!(shows(decide_media(&clip.to_string_lossy(), Origin::ThisMac)), Some(clip.clone()));
        // An older QuickTime movie: padding, its data, then the movie box.
        let old = bytes(&s, "old.MOV", b"\0\0\0\x08wide\0\0\0\x10mdat12345678\0\0\0\x10moov\0\0\0\x08mvhd");
        assert_eq!(shows(decide_media(&old.to_string_lossy(), Origin::ThisMac)), Some(old));
        let pdf = bytes(&s, "spec.pdf", b"%PDF-1.7\n");
        assert_eq!(shows(decide_media(&pdf.to_string_lossy(), Origin::ThisMac)), Some(pdf));
    }

    /// What's shown is the file that was checked, held open, read from its
    /// start: replacing the path afterwards changes nothing it reads.
    #[test]
    fn the_file_checked_is_the_one_read() {
        use std::io::Read;
        let s = Scratch::new("media-held");
        let clip = bytes(&s, "hero.mp4", MP4_HEAD);
        let MediaVerdict::Show(mut checked) = decide_media(&clip.to_string_lossy(), Origin::ThisMac) else { panic!() };
        fs::remove_file(&clip).unwrap();
        fs::write(&clip, b"something else entirely").unwrap();
        let mut read = Vec::new();
        checked.file.read_to_end(&mut read).unwrap();
        assert_eq!(read, MP4_HEAD);
        assert_eq!(checked.len, MP4_HEAD.len() as u64);
    }

    #[test]
    fn another_macs_video_never_plays_here() {
        let s = Scratch::new("media-other");
        let clip = bytes(&s, "hero.mp4", MP4_HEAD);
        // The same path is here too, but it isn't the file that thread means.
        assert!(refusal(decide_media(&clip.to_string_lossy(), Origin::OtherMac)).contains("on another Mac"));
    }

    #[test]
    fn only_a_real_video_plays() {
        let s = Scratch::new("media-fake");
        // Named like a film, but text, or a folder.
        let text = bytes(&s, "notes.mp4", b"#!/bin/sh\nrm -rf ~\n");
        assert!(refusal(decide_media(&text.to_string_lossy(), Origin::ThisMac)).contains("isn't really a video"));
        let folder = s.path("folder.mp4");
        fs::create_dir_all(&folder).unwrap();
        refusal(decide_media(&folder.to_string_lossy(), Origin::ThisMac));
        let fake_pdf = bytes(&s, "fake.pdf", b"<html>");
        assert!(refusal(decide_media(&fake_pdf.to_string_lossy(), Origin::ThisMac)).contains("isn't really a PDF"));
        // Not a video or a PDF by name at all.
        let key = bytes(&s, "id_ed25519", MP4_HEAD);
        refusal(decide_media(&key.to_string_lossy(), Origin::ThisMac));
        assert!(refusal(decide_media(&s.path("gone.mp4").to_string_lossy(), Origin::ThisMac)).contains("isn't there"));
        refusal(decide_media("clips/hero.mp4", Origin::ThisMac));
    }

    /// `ftyp` four letters in isn't enough (Codex's review, 12 Nov): a text
    /// file that says it there has no sane box size, and a HEIF or AVIF
    /// picture renamed `.mov` has a picture's brand.
    #[test]
    fn a_box_that_only_looks_like_a_films_isnt_one() {
        let s = Scratch::new("media-boxes");
        let text = bytes(&s, "fake.mp4", b"xxxxftyphello, this is only text");
        refusal(decide_media(&text.to_string_lossy(), Origin::ThisMac));
        let avif = bytes(&s, "photo.mov", b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf\0\0\0\0");
        refusal(decide_media(&avif.to_string_lossy(), Origin::ThisMac));
        let heic = bytes(&s, "photo.mp4", b"\0\0\0\x18ftypheic\0\0\0\0mif1heic");
        refusal(decide_media(&heic.to_string_lossy(), Origin::ThisMac));
        // A box longer than the file it's in.
        let short = bytes(&s, "short.mov", b"\0\0\x10\0moov\0\0\0\0");
        refusal(decide_media(&short.to_string_lossy(), Origin::ThisMac));
        let screen_recording = bytes(&s, "Screen Recording.mov", b"\0\0\0\x14ftypqt  \0\0\0\0qt  \0\0\0\x08wide");
        assert!(shows(decide_media(&screen_recording.to_string_lossy(), Origin::ThisMac)).is_some());
        assert!(!looks_like(Media::Pdf, b"%PDF-x", 6));
    }

    /// The boxes in front of the first that says what the file is are walked
    /// past, not trusted (Codex's second review, 12 Nov): padding in front
    /// of a picture's `ftyp` doesn't make it a film, nor does a 64-bit size
    /// that's nonsense, a run of padding with no movie after it, or a
    /// picture's brand among a film's compatible brands.
    #[test]
    fn padding_in_front_doesnt_make_a_picture_a_film() {
        let avif: &[u8] = b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf\0\0\0\0";
        let free8: &[u8] = b"\0\0\0\x08free";
        let padded = [free8, avif].concat();
        assert!(!looks_like(Media::Video, &padded, padded.len() as u64));
        let skipped = [b"\0\0\0\x0cskip\0\0\0\0".as_slice(), free8, avif].concat();
        assert!(!looks_like(Media::Video, &skipped, skipped.len() as u64));
        let s = Scratch::new("media-padded");
        let file = bytes(&s, "photo.mov", &padded);
        assert!(refusal(decide_media(&file.to_string_lossy(), Origin::ThisMac)).contains("isn't really a video"));

        // A film's brand first, a picture's among the rest.
        let mixed: &[u8] = b"\0\0\0\x18ftypisom\0\0\0\0isommif1";
        assert!(!looks_like(Media::Video, mixed, mixed.len() as u64));

        // 64-bit sizes: one smaller than its own header, one past the end,
        // and a sane one in front of a film's ftyp.
        let tiny: &[u8] = b"\0\0\0\x01free\0\0\0\0\0\0\0\x04\0\0\0\x08moov";
        assert!(!looks_like(Media::Video, tiny, tiny.len() as u64));
        let huge = [b"\0\0\0\x01mdat\x7f\xff\xff\xff\xff\xff\xff\xff".as_slice(), b"\0\0\0\x08moov"].concat();
        assert!(!looks_like(Media::Video, &huge, huge.len() as u64));
        let wide_mdat = [b"\0\0\0\x01mdat\0\0\0\0\0\0\0\x14abcd".as_slice(), MP4_HEAD].concat();
        assert!(looks_like(Media::Video, &wide_mdat, wide_mdat.len() as u64));

        // Padding and nothing after it, or padding that runs to the end.
        let only_padding = [free8, free8].concat();
        assert!(!looks_like(Media::Video, &only_padding, only_padding.len() as u64));
        let to_end: &[u8] = b"\0\0\0\0free\0\0\0\x08moov";
        assert!(!looks_like(Media::Video, to_end, to_end.len() as u64));
        // More padding boxes than are ever walked past.
        let many = [free8.repeat(LEADING_BOXES), b"\0\0\0\x08moov".to_vec()].concat();
        assert!(!looks_like(Media::Video, &many, many.len() as u64));
        // A box type nothing starts with.
        let other: &[u8] = b"\0\0\0\x08meta\0\0\0\x08moov";
        assert!(!looks_like(Media::Video, other, other.len() as u64));

        // Films that really start with padding still play.
        let padded_film = [free8, MP4_HEAD].concat();
        assert!(looks_like(Media::Video, &padded_film, padded_film.len() as u64));
        let moov_first: &[u8] = b"\0\0\0\x10moov\0\0\0\x08mvhd";
        assert!(looks_like(Media::Video, moov_first, 16));
        let moov_then_data = [moov_first, b"\0\0\0\x0cmdatdata".as_slice(), b"\0\0\0\x0cudta\0\0\0\0"].concat();
        assert!(looks_like(Media::Video, &moov_then_data, moov_then_data.len() as u64));

        // A movie box in front doesn't make a picture a film either (Codex's
        // third review, 12 Nov), and an empty one isn't a movie.
        let moov_avif = [moov_first, avif].concat();
        assert!(!looks_like(Media::Video, &moov_avif, moov_avif.len() as u64));
        let empty_moov_avif = [b"\0\0\0\x08moov".as_slice(), avif].concat();
        assert!(!looks_like(Media::Video, &empty_moov_avif, empty_moov_avif.len() as u64));
        assert!(!looks_like(Media::Video, b"\0\0\0\x08moov", 8));
        // Nor does anything between the movie box and the picture's ftyp
        // (Codex's fourth review, 12 Nov): user data, or padding enough to
        // use up the walk.
        let udta_avif = [moov_first, b"\0\0\0\x08udta".as_slice(), avif].concat();
        assert!(!looks_like(Media::Video, &udta_avif, udta_avif.len() as u64));
        let padded_avif = [moov_first, &free8.repeat(LEADING_BOXES), avif].concat();
        assert!(!looks_like(Media::Video, &padded_avif, padded_avif.len() as u64));
        let endless = [moov_first, &free8.repeat(ALL_BOXES)].concat();
        assert!(!looks_like(Media::Video, &endless, endless.len() as u64));
        let movie_with_extras = [moov_first, &free8.repeat(LEADING_BOXES), b"\0\0\0\x0cudta\0\0\0\0"].concat();
        assert!(looks_like(Media::Video, &movie_with_extras, movie_with_extras.len() as u64));
    }

    /// A pipe named like a film is refused at once, not waited on: opening
    /// one to read would hold the app until something wrote to it.
    #[cfg(unix)]
    #[test]
    fn a_pipe_named_like_a_video_is_refused_without_waiting() {
        let s = Scratch::new("media-pipe");
        let pipe = s.path("stream.mp4");
        let c = std::ffi::CString::new(pipe.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let path = pipe.to_string_lossy().into_owned();
        std::thread::spawn(move || tx.send(shows(decide_media(&path, Origin::ThisMac))).unwrap());
        let got = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("decide_media waited on a pipe");
        assert_eq!(got, None);
    }

    /// A link is judged by what it leads to: a `.mp4` link to a film plays
    /// that film, and one to anything else (even with a film's first bytes)
    /// doesn't.
    #[cfg(unix)]
    #[test]
    fn a_link_named_like_a_video_is_judged_by_what_it_leads_to() {
        let s = Scratch::new("media-link");
        let real = bytes(&s, "take2.mov", MP4_HEAD);
        let link = s.path("latest.mp4");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(shows(decide_media(&link.to_string_lossy(), Origin::ThisMac)), Some(real));
        let secret = bytes(&s, "secret.key", MP4_HEAD);
        let posing = s.path("clip.mp4");
        std::os::unix::fs::symlink(&secret, &posing).unwrap();
        refusal(decide_media(&posing.to_string_lossy(), Origin::ThisMac));
    }

    #[test]
    fn media_kinds_by_name() {
        assert_eq!(media_kind("/a/b/Launch.MP4"), Some(Media::Video));
        assert_eq!(media_kind("/a/b.m4v"), Some(Media::Video));
        assert_eq!(media_kind("/a/b.pdf"), Some(Media::Pdf));
        assert_eq!(media_kind("/a/b.webm"), None);
        assert_eq!(media_kind("/a/.mp4"), None);
        assert!(!looks_like(Media::Video, b"\0\0\0", 3));
    }

    #[test]
    fn absolute_paths() {
        assert_eq!(absolute("docs/a.md", "/w/repo/", "/h"), Some("/w/repo/docs/a.md".into()));
        assert_eq!(absolute("./a.md", "/w", "/h"), Some("/w/a.md".into()));
        assert_eq!(absolute("~/a.md", "/w", "/h/"), Some("/h/a.md".into()));
        assert_eq!(absolute("/x", "", "/h"), Some("/x".into()));
        assert_eq!(absolute("a.md", "", "/h"), None);
    }
}
