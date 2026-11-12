// Nothing in the desktop apps hands a link or a file to the system except the
// open gate (desktop/gpui/src/open_gate.rs), as OpenGateSourceTests is for the
// Swift app. Each of Daybreak's findings 58, 59 and 64 (9 Nov 2026) was a new
// button or link that called the system's opener itself, with a URL made by
// pasting a path into `file://`.
use std::path::{Path, PathBuf};

/// What reaches the system's opener, or starts a program, directly. The
/// kit's own rich text, links and menu links count: with no handler of their
/// own they call `open_url` themselves (Codex review of the 10 Nov fix), so
/// they're made only by the gate's `text_view` and `reading_markdown_view`.
const OPENERS: &[&str] = &[
    ".open_url(",
    ".reveal_path(",
    "open_with_system",
    "Command::new(",
    // The terminal library's own way of starting a program in a pty.
    "tty::new(",
    "NSWorkspace",
    "ShellExecute",
    "xdg-open",
    "format!(\"file://",
    "TextView::new(",
    "TextView::markdown(",
    "TextView::html(",
    "PopupMenuItem::link(",
    "Link::new(",
    "EditorHost",
    // The Mac's own video and PDF views (gpui/native/media_views.m): what
    // makes one reads the file, so only the gate's `show_media` does.
    "tb_media_open",
    // And nothing in the Objective-C beside the app opens anything either.
    "openURL:",
    "openFile:",
];

/// The gate itself, a test helper that starts the mock runner, and the
/// connection bar's Start runner, which runs launchctl on the runner's own
/// LaunchAgent (and `id -u` for the user) with fixed arguments: nothing a link,
/// a file or the runner names.
const ALLOWED: &[&str] = &["gpui/src/open_gate.rs", "client/tests/mock_runner.rs", "gpui/src/runner_launcher.rs"];

/// Single lines elsewhere, each by its file and the text it holds, allowed
/// exactly as many times as listed, so anything else in the same file is
/// still caught (Codex's review, 11 Nov: the whole Terminal file was let off).
/// The Terminal panel starts your login shell (`$SHELL -l` through `env -u`,
/// threadbear_core::terminal::launch) in a pty in a thread's folder or your
/// home folder, as Terminal would, and runs nothing a reply, a link or a file
/// says (a shell block's command is typed at its prompt for you to run).
/// The hang watch runs macOS's `sample` on the app itself, inside
/// `sandbox-exec` with core's fixed profile (hang_sample.rs), and nothing else.
/// The first launch reads the Swift app's settings with `defaults export` of
/// its own domain, once (swift_import.rs). The self-update (app_update.rs)
/// reads main's tree with git, runs the update script from the app's own
/// bundle under `taskpolicy -b`, and opens the installed app again once this
/// one has quit: fixed programs, on the app's own checkout and bundle.
/// The Objective-C beside the app (its native/ folders) is read too: the
/// video and PDF views are made in one, and only the gate asks for one.
const ALLOWED_LINES: &[(&str, &str, usize)] = &[
    ("gpui/src/terminal.rs", "let pty = tty::new(&options, self.size.window_size(), 0)?;", 1),
    ("gpui/src/hang_watch.rs", "let mut sample = Command::new(\"/usr/bin/sandbox-exec\");", 1),
    ("gpui/src/app_update.rs", "let started = Command::new(\"/bin/sh\").arg(\"-c\").arg(wait).arg(\"sh\").args(open).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();", 1),
    ("gpui/src/app_update.rs", "let out = Command::new(\"/usr/bin/git\").args(rules::rev_parse_arguments(checkout)).stdin(Stdio::null()).output().ok()?;", 1),
    ("gpui/src/app_update.rs", "let child = Command::new(program).args(arguments).current_dir(installed::home_dir()).stdin(Stdio::null()).stdout(out).stderr(err).spawn();", 1),
    // Where the video and PDF views are made, called only from the gate.
    ("gpui/native/media_views.m", "void *tb_media_open(void *nativeParent, int fd, long long length, const char *path, int kind, bool muted, float volume) {", 1),
    ("gpui/src/swift_import.rs", "let out = Command::new(\"/usr/bin/defaults\").args([\"export\", \"com.mikerosoft.threadbear\", \"-\"]).stderr(Stdio::null()).output().map_err(|e| format!(\"couldn't run defaults: {e}\"))?;", 1),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target" || n == "golden") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs" || e == "m") {
            out.push(path);
        }
    }
}

#[test]
fn nothing_opens_outside_the_gate() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    // web/ too: the web runtime's pages are where a link would most want out.
    for crate_dir in ["gpui", "dock", "client", "core/src", "web"] {
        rust_files(&desktop.join(crate_dir), &mut files);
    }
    assert!(files.iter().any(|f| f.ends_with("gpui/src/open_gate.rs")), "the gate wasn't found, so this test isn't looking in the right place");
    let mut found = Vec::new();
    let mut allowed_seen = vec![0; ALLOWED_LINES.len()];
    for file in &files {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if let Some(i) = ALLOWED_LINES.iter().position(|(f, l, _)| *f == rel && line.trim() == *l) {
                allowed_seen[i] += 1;
                continue;
            }
            // Comments may name them; `file://` itself has a `//` in it, so
            // only whole comment lines are passed over.
            if line.trim_start().starts_with("//") {
                continue;
            }
            for opener in OPENERS {
                if line.contains(opener) {
                    found.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "open links and files through open_gate.rs (open_web, open_link, open_file, reveal), not directly:\n{}", found.join("\n"));
    for ((file, line, times), seen) in ALLOWED_LINES.iter().zip(allowed_seen) {
        assert_eq!(seen, *times, "{file} should have `{line}` {times} time(s): if it has moved or changed, say why it's safe and change ALLOWED_LINES");
    }
}

/// The gate's own sink is the only `open_url` in it, so everything in it
/// goes through the checks above it.
#[test]
fn the_gate_opens_in_one_place() {
    let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/open_gate.rs");
    let text = std::fs::read_to_string(gate).unwrap();
    assert_eq!(text.matches(".open_url(").count(), 1);
    assert!(!text.contains("format!(\"file://"));
}

#[test]
fn reply_link_probes_use_the_local_file_gate() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/reply_links.rs");
    let text = std::fs::read_to_string(file).unwrap();
    // A probe or a hover runs before a click has reached the opener. On
    // Windows exists/is_dir/open can already contact an SMB server.
    for unsafe_probe in ["Path::new(p).exists()", "Path::new(&file.path).exists()", "Path::new(&file.path).is_dir()", "Path::new(&path).is_dir()", "std::fs::File::open(path)"] {
        assert!(!text.contains(unsafe_probe), "reply link bypasses the local path gate: {unsafe_probe}");
    }
    assert!(text.contains("policy::local_metadata"));
    assert!(text.contains("policy::local_read"));
    assert!(!text.contains("FilePicture::Image(crate::rows::markdown::picture_file("), "hover pictures must use the guarded Windows loader too");
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/text_file.rs");
    let text = std::fs::read_to_string(core).unwrap();
    let production = text.split("#[cfg(test)]").next().unwrap();
    assert!(!production.contains("fs::File::open("));
    assert!(!production.contains("fs::OpenOptions::new("));
    assert!(production.contains("open_policy::local_open("));
    assert!(production.contains("open_policy::local_open_for_write("));
    let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/open_gate.rs");
    let text = std::fs::read_to_string(gate).unwrap();
    let reveal = text.split("pub(crate) fn reveal(").nth(1).unwrap().split("// ----------------------------------------------------------------").next().unwrap();
    assert!(reveal.contains("policy::with_local_path"));
    let watcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/file_watch.rs");
    assert!(std::fs::read_to_string(watcher).unwrap().contains("open_policy::with_local_path"));
    let folders = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/folders.rs");
    let text = std::fs::read_to_string(folders).unwrap();
    assert!(!text.contains("ImageReader::open("));
    assert!(!text.contains("image::open("));
    assert!(text.contains("open_policy::local_read("));
}

/// The Mac's own video and PDF views read only the file the gate checked and
/// send their links back to it (Codex's review of the views, 12 Nov):
/// - a `PDFView` with no delegate of its own hands a clicked link to
///   NSWorkspace, so every one is the app's subclass with a delegate that
///   takes the link callbacks, and its own `performAction:`;
/// - a film is never opened by its path or URL (`playerWithURL:`,
///   `playerItemWithURL:`, an asset made from a file URL), which would read
///   whatever's there by then, but from the held file through the asset made
///   with QuickTime's references to other files forbidden.
///
/// And from Codex's second review, the same day:
/// - PDFKit's right-click menu opens a link itself (Open Link, Open With), so
///   a PDF view's menu goes through `tb_safe_menu` both as it's made and as
///   it opens;
/// - a film dragged out is a copy of the held file, never a URL made from
///   its path, which whatever it's dropped on would read later;
/// - a PDF's bytes are read off the main thread, never as the pane draws;
/// - nothing there calls NSWorkspace at all.
#[test]
fn the_media_views_read_only_what_the_gate_checked() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    for crate_dir in ["gpui", "dock", "client", "core/src", "web"] {
        rust_files(&desktop.join(crate_dir), &mut files);
    }
    let mut found = Vec::new();
    for file in files.iter().filter(|f| f.extension().is_some_and(|e| e == "m")) {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(file).unwrap();
        let code: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
        let has = |needle: &str| code.iter().any(|l| l.contains(needle));
        for line in &code {
            for opener in ["playerWithURL:", "playerItemWithURL:", "AVPlayer alloc] initWithURL:", "AVPlayerItem alloc] initWithURL:"] {
                if line.contains(opener) {
                    found.push(format!("{rel}: plays by path: {}", line.trim()));
                }
            }
            if line.contains("URLAssetWithURL:") && !line.contains("AVURLAssetReferenceRestrictionsKey : @(AVAssetReferenceRestrictionForbidAll)") {
                found.push(format!("{rel}: an asset without its references forbidden: {}", line.trim()));
            }
            if line.contains("[PDFView alloc]") {
                found.push(format!("{rel}: a plain PDFView, whose links go to the system: {}", line.trim()));
            }
            if line.contains("NSWorkspace") {
                found.push(format!("{rel}: opens through NSWorkspace, not the gate: {}", line.trim()));
            }
            if line.contains("fileURLWithPath:self.path") || line.contains("fileURLWithPath:file") {
                found.push(format!("{rel}: hands out the real file's path, which can be swapped before it's read: {}", line.trim()));
            }
        }
        if has("PDFView") {
            for needed in [
                "- (void)PDFViewWillClickOnLink:",
                "- (void)PDFViewOpenPDF:",
                "- (void)performAction:(PDFAction *)action",
                "pdf.delegate = ",
                "- (NSMenu *)menuForEvent:(NSEvent *)event {",
                "- (void)willOpenMenu:(NSMenu *)menu withEvent:(NSEvent *)event {",
            ] {
                if !has(needed) {
                    found.push(format!("{rel}: a PDF view without `{needed}`, so its links could reach the system"));
                }
            }
            // Each of those menu methods puts the menu through the filter
            // before anything else sees it.
            for (i, line) in code.iter().enumerate() {
                if line.contains("- (NSMenu *)menuForEvent:") || line.contains("- (void)willOpenMenu:") {
                    let body: String = code[i..(i + 4).min(code.len())].join("\n");
                    if !body.contains("tb_safe_menu(menu, self.links);") {
                        found.push(format!("{rel}: a PDF menu that isn't put through tb_safe_menu: {}", line.trim()));
                    }
                }
            }
        }
        // A whole PDF read only off the main thread.
        for (i, line) in code.iter().enumerate() {
            if line.contains("tb_read_all(") && !line.contains("static NSData *tb_read_all(") {
                let before: String = code[i.saturating_sub(3)..i].join("\n");
                if !before.contains("dispatch_get_global_queue(") {
                    found.push(format!("{rel}: a PDF read where the pane draws, not off the main thread: {}", line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "the media views must read only the gate's file and send their links back to it:\n{}", found.join("\n"));
}
