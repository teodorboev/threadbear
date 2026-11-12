// Every file the desktop apps write goes through private_file.rs (owner-only
// from its first byte, whole, never through a file or symlink someone made
// first), except the few below that aren't this user's private data. Daybreak's
// findings 88 and 94 (11 Nov 2026) were two writers that each wrote a fixed
// `<name>.json.tmp` with `fs::write` and only then chmodded it; a third
// writer, added the same day, wrote its file directly.
use std::path::{Path, PathBuf};

/// What creates or writes a file, or changes its permissions, directly.
const WRITERS: &[&str] = &["fs::write(", "File::create(", "OpenOptions::new(", "set_permissions(", "DirBuilder::new("];

/// Whole files let off:
/// - the helper itself;
/// - the web runtime's install (Chromium's framework, its helper apps and
///   their Info.plists, unpacked from a pinned download): programs, not data;
/// - a file of the user's own opened in the side pane and saved back in
///   place, which keeps the permissions that file already had;
/// - the dock's demo program, which isn't part of the app.
const ALLOWED: &[&str] = &["core/src/private_file.rs", "core/src/web_runtime.rs", "core/src/text_file.rs", "dock/src/bin/dock-demo/main.rs", "dock/src/bin/dock-demo/script.rs"];

/// Single lines elsewhere, each by its file and the text it holds, allowed
/// exactly as many times as listed:
/// - a pasted picture's own folder and file, made 0700 and 0600 from the
///   start with `create_new` (`stage_private`, Daybreak finding 63);
/// - the automation's web runtime smoke test saving its picture where the
///   spec asked (debug builds only);
/// - the open gate opening a file it has checked, to read only
///   (`open_without_waiting`: read(true), O_NOFOLLOW, nothing written).
/// - an existing user file saved in place by the text editor, keeping its mode.
const ALLOWED_LINES: &[(&str, &str, usize)] = &[
    ("gpui/src/composer.rs", "let mut options = std::fs::OpenOptions::new();", 1),
    ("gpui/src/web_runtime.rs", "std::fs::write(path, &png).map_err(|e| format!(\"couldn't save the picture: {e}\"))?;", 1),
    ("core/src/open_policy.rs", "let mut options = std::fs::OpenOptions::new();", 1),
    ("core/src/open_policy.rs", "{ std::fs::OpenOptions::new().write(true).open(path) }", 1),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Test folders write fixtures as they like.
            if path.file_name().is_some_and(|n| n == "target" || n == "golden" || n == "tests") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_file_is_written_through_private_file() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    for crate_dir in ["gpui/src", "dock/src", "client/src", "core/src", "web/src"] {
        rust_files(&desktop.join(crate_dir), &mut files);
    }
    assert!(files.iter().any(|f| f.ends_with("gpui/src/drafts.rs")), "the drafts weren't found, so this test isn't looking in the right place");
    let mut found = Vec::new();
    let mut allowed_seen = vec![0; ALLOWED_LINES.len()];
    for file in &files {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let mut cfg_test = false;
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            // A file's unit tests come last, after `#[cfg(test)]`, and write
            // their fixtures as they like.
            if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[cfg(all(test") {
                cfg_test = true;
            }
            if cfg_test && trimmed.starts_with("mod ") && trimmed.ends_with('{') {
                break;
            }
            if !trimmed.starts_with("#[") {
                cfg_test = false;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            if let Some(i) = ALLOWED_LINES.iter().position(|(f, l, _)| *f == rel && trimmed == *l) {
                allowed_seen[i] += 1;
                continue;
            }
            for writer in WRITERS {
                if line.contains(writer) {
                    found.push(format!("{rel}:{}: {}", n + 1, trimmed));
                }
            }
        }
    }
    assert!(found.is_empty(), "write files through threadbear_core::private_file, not directly:\n{}", found.join("\n"));
    for ((file, line, times), seen) in ALLOWED_LINES.iter().zip(allowed_seen) {
        assert_eq!(seen, *times, "{file} should have `{line}` {times} time(s): if it has moved or changed, say why it's safe and change ALLOWED_LINES");
    }
}
