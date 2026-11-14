// What a local HTML file previewed in the pane (or on the review window's
// stage) may read: itself, and the styles, pictures, fonts and media it names,
// in its own folder or below. Nothing else beside it, so a page from a project,
// or one an agent was talked into writing, can't read a `.env`, a key or a
// database kept in the same folder. A port of the Swift app's LocalPreviewSite
// (Daybreak finding 6 and its reviews), which the web runtime's preview pages
// ask for every request (desktop/web, mac/local.rs).
//
// A file is served only when all of these hold:
// - its path has no `..`, no empty part, no part starting with a dot, no
//   control character, backslash or `%`;
// - it's the page itself, or a passive asset (`ASSET_EXTENSIONS`): styles,
//   pictures, fonts, sound and video. Never another page, a script, SVG
//   (which can carry script), `.json`, `.txt`, `.env`, keys...;
// - a page or stylesheet served before names it in markup or CSS (the page
//   itself always): `src`, `href`, `poster`, `data`, `srcset`, a style's
//   `url()`, a stylesheet's `url()` and `@import`. Never a string in a script;
// - it's reached from the folder without following a symlink anywhere on the
//   way, and is a regular file with one link, checked on the open file it's
//   then read from.
//
// The page can still name a file beside it and show it, so the page's view lets
// nothing out (no network, no navigating away, no windows, no downloads), and
// its scripts are off unless Mike chose Run them for those exact bytes
// (`ScriptConsent`, and `Site::page_digest`).
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a page can pull in besides itself: passive files only. A sibling page
/// or script named by the page would let it read whatever that names in turn.
pub const ASSET_EXTENSIONS: &[&str] = &[
    "css", "png", "jpg", "jpeg", "gif", "webp", "avif", "ico", "bmp", "heic", "woff", "woff2", "ttf", "otf", "mp4", "webm", "m4v", "mov", "mp3", "m4a", "wav", "ogg",
];
/// What can be the page.
pub const PAGE_EXTENSIONS: &[&str] = &["html", "htm"];
/// Files read for what they name: the page and stylesheets.
const SCANNED_EXTENSIONS: &[&str] = &["html", "htm", "css"];
/// Bigger than any page or picture a preview needs.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// How much of a page or stylesheet is read for names, and how many are kept
/// from one file and in all (a 64 MB page of short `src=` made millions).
pub const SCAN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REFERENCES_PER_FILE: usize = 2_000;
pub const MAX_ALLOWED: usize = 10_000;
/// The longest path kept, decoded. A Mac path stops at 1,024 bytes, so a
/// longer one names nothing a page could read. The counts above didn't bound
/// the bytes: 2,000 names of megabytes each made 15.6 GiB from a 4 MiB page
/// (Daybreak 118, 13 Nov 2026).
pub const MAX_REFERENCE_BYTES: usize = 1_024;
/// The longest name read from a page, as written: room for a 1,024-byte path
/// with every byte `%XX` (Japanese folder names, say). Longer is skipped,
/// never copied (Codex's review).
pub const MAX_REFERENCE_SOURCE_BYTES: usize = 3 * MAX_REFERENCE_BYTES + 64;
/// What all the names a site keeps come to, in bytes: thousands of real
/// asset paths, a few megabytes at the very most.
pub const MAX_ALLOWED_BYTES: usize = 2 * 1024 * 1024;
/// How much of a page `has_scripts` reads.
pub const SCRIPT_SCAN_BYTES: usize = 4 * 1024 * 1024;

/// The words over a page with scripts that aren't running (`LocalPagePreview`).
pub mod words {
    pub const HAS_SCRIPTS: &str = "This page has scripts";
    pub const RUN_THEM: &str = "Run them";
    pub const RUN_THEM_TIP: &str = "Reload this page with its scripts on";
    pub const EXPLANATION: &str = "Pages open here with their scripts off, as a script could read the files the page names. Run them turns them on for this version of the page, still with no network and nothing read but what it names. If the file changes, you'll be asked again.";
}

fn extension(name: &str) -> String {
    let last = name.rsplit('/').next().unwrap_or(name);
    match last.rfind('.') {
        Some(i) if i > 0 => last[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Whether a file name is one a preview shows as a page.
pub fn is_page(name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name);
    !last.starts_with('.') && PAGE_EXTENSIONS.contains(&extension(last).as_str())
}

/// A file served: its bytes and its MIME type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub data: Vec<u8>,
    pub mime: String,
}

/// Whether a local page may show in the app (the open gate's rule for it, as
/// `open_policy::decide_media` is for videos and PDFs), and the real file it
/// is: only this Mac's own, after its symlinks are followed, and only a real
/// file whose real name is a page's, so `notes.html` linked to a `.env` is
/// refused. What the page then reads is the `Site`'s to say.
pub fn decide_page(path: &str, origin: crate::open_policy::Origin) -> Result<PathBuf, String> {
    let given = crate::highlights::last_component(path);
    let name = if given.is_empty() { "That file" } else { given };
    if origin == crate::open_policy::Origin::OtherMac {
        return Err(format!("{name} is on another Mac, so it can't show here yet."));
    }
    let given_path = Path::new(path);
    if !given_path.is_absolute() {
        return Err(format!("Threadbear couldn't tell where {name} is."));
    }
    let Ok(real) = crate::open_policy::real_path(given_path) else {
        return Err(format!("{name} isn't there any more."));
    };
    let real_name = real.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let is_file = std::fs::metadata(&real).is_ok_and(|m| m.is_file());
    if !is_page(real_name) || safe_parts(real_name).is_none() || !is_file {
        return Err(format!("{name} isn't really a web page, so Threadbear didn't show it."));
    }
    Ok(real)
}

/// One page's folder and what it may read so far.
pub struct Site {
    /// The page's folder, symlinks resolved.
    pub root: PathBuf,
    /// The page's file name in `root`.
    pub page: String,
    /// The page's bytes Mike allowed to run scripts (SHA-256, hex): the page
    /// is served only while its bytes still match, so a page rewritten after
    /// the click gets nothing.
    pub page_digest: Option<String>,
    /// What it may read, and their names' bytes together (`MAX_ALLOWED_BYTES`).
    allowed: Mutex<(HashSet<String>, usize)>,
    /// The folder, held open for the site's life: every read starts here, so
    /// the folder being moved or swapped for a link later changes nothing.
    #[cfg(unix)]
    root_fd: std::os::fd::OwnedFd,
}

impl Site {
    /// A site for `file`, a page (its symlinks already followed by the
    /// caller, `decide_page`). None for anything that isn't a page, or whose
    /// folder can't be opened.
    pub fn new(file: &Path, page_digest: Option<String>) -> Option<Site> {
        let page = file.file_name()?.to_str()?.to_owned();
        if !is_page(&page) || safe_parts(&page).is_none() {
            return None;
        }
        let root = dunce::canonicalize(file.parent()?).ok()?;
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd as _;
            use std::os::unix::ffi::OsStrExt as _;
            let c = std::ffi::CString::new(root.as_os_str().as_bytes()).ok()?;
            let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
            if fd < 0 {
                return None;
            }
            let root_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
            Some(Site { root, allowed: Mutex::new((HashSet::from([page.clone()]), page.len())), page, page_digest, root_fd })
        }
        #[cfg(not(unix))]
        {
            let _ = (root, page, page_digest);
            None
        }
    }

    /// Whether `relative` is one the page may read now (for tests).
    pub fn allows(&self, relative: &str) -> bool {
        self.allowed.lock().is_ok_and(|a| a.0.contains(relative))
    }

    pub fn allowed_count(&self) -> usize {
        self.allowed.lock().map_or(0, |a| a.0.len())
    }

    /// The bytes and type for `path` (from the folder, decoded, as the page
    /// asked for it), or None when it isn't one the page may read.
    pub fn serve(&self, path: &str) -> Option<Served> {
        let path = path.strip_prefix('/').unwrap_or(path);
        let parts = safe_parts(path)?;
        let relative = parts.join("/");
        let ext = extension(&relative);
        let is_page = relative == self.page;
        if !is_page && !(ASSET_EXTENSIONS.contains(&ext.as_str()) && self.allows(&relative)) {
            return None;
        }
        let data = self.read(&parts)?;
        if is_page {
            if let Some(wanted) = &self.page_digest {
                if &digest(&data) != wanted {
                    return None;
                }
            }
        }
        let scanned = SCANNED_EXTENSIONS.contains(&ext.as_str());
        let utf8 = scanned && std::str::from_utf8(&data).is_ok();
        if scanned {
            let head = &data[..data.len().min(SCAN_BYTES)];
            // A character cut in two at the end reads as U+FFFD, which names nothing.
            let text = String::from_utf8_lossy(head);
            let dir = relative.rsplit_once('/').map_or("", |(d, _)| d);
            let refs = if ext == "css" { css_references(&text, MAX_REFERENCES_PER_FILE) } else { html_references(&text, MAX_REFERENCES_PER_FILE) };
            if let Ok(mut allowed) = self.allowed.lock() {
                let (allowed, bytes) = &mut *allowed;
                for found in refs.iter().filter_map(|r| resolve(r, dir)) {
                    if allowed.len() >= MAX_ALLOWED || *bytes + found.len() > MAX_ALLOWED_BYTES {
                        break;
                    }
                    // A name joined to its folder can come out longer than it went in.
                    if found.len() > MAX_REFERENCE_BYTES || allowed.contains(&found) {
                        continue;
                    }
                    *bytes += found.len();
                    allowed.insert(found);
                }
            }
        }
        let mut mime = mime_type(&ext).to_owned();
        // With no charset a UTF-8 page's ✓ shows as "âœ“".
        if utf8 {
            mime.push_str("; charset=utf-8");
        }
        Some(Served { data, mime })
    }

    /// The file at `parts` under the open folder: each folder on the way and
    /// the file itself opened with O_NOFOLLOW, so a symlink anywhere refuses
    /// it, then checked and read on that one open file.
    #[cfg(unix)]
    fn read(&self, parts: &[String]) -> Option<Vec<u8>> {
        use std::io::Read as _;
        use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
        let (name, folders) = parts.split_last()?;
        let mut held: Vec<OwnedFd> = vec![];
        let mut dir = self.root_fd.as_raw_fd();
        for part in folders {
            let c = std::ffi::CString::new(part.as_bytes()).ok()?;
            let fd = unsafe { libc::openat(dir, c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
            if fd < 0 {
                return None;
            }
            let owned = unsafe { OwnedFd::from_raw_fd(fd) };
            dir = owned.as_raw_fd();
            held.push(owned);
        }
        let c = std::ffi::CString::new(name.as_bytes()).ok()?;
        // O_NONBLOCK, so a FIFO named like a picture can't hang the open.
        let fd = unsafe { libc::openat(dir, c.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let meta = file.metadata().ok()?;
        use std::os::unix::fs::MetadataExt as _;
        // A hard link is the same file as one somewhere else.
        if !meta.file_type().is_file() || meta.nlink() != 1 || meta.len() > MAX_BYTES {
            return None;
        }
        let mut out = Vec::with_capacity(meta.len() as usize);
        // It may have grown after the check: one byte past the limit says so.
        file.take(MAX_BYTES + 1).read_to_end(&mut out).ok()?;
        if out.len() as u64 > MAX_BYTES {
            return None;
        }
        Some(out)
    }

    #[cfg(not(unix))]
    fn read(&self, _parts: &[String]) -> Option<Vec<u8>> {
        None
    }
}

/// The type a file is served as, by its extension.
pub fn mime_type(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/vnd.microsoft.icon",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "m4v" => "video/x-m4v",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

/// The path's parts, or None for one with `..`, `.`, an empty part or a
/// dotfile, or with a control character, a backslash or a `%` anywhere: a NUL
/// would cut the name short at the open, and a `%` left after decoding means
/// it was encoded twice, which no real asset name needs.
pub fn safe_parts(path: &str) -> Option<Vec<String>> {
    if path.chars().any(|c| c.is_control() || c == '\\' || c == '%') {
        return None;
    }
    let parts: Vec<String> = path.split('/').map(str::to_owned).collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty() || p.starts_with('.')) {
        return None;
    }
    Some(parts)
}

/// `%XX` decoded, or None for a bad escape or bytes that aren't UTF-8 (as
/// `removingPercentEncoding` gives nil).
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = |b: u8| (b as char).to_digit(16);
            let (Some(hi), Some(lo)) = (bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b))) else { return None };
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `reference`, named in a file in `dir`, as a path from the folder: None for
/// anything with a scheme (`https:`, `data:`), a `//host`, or a path that
/// climbs out of the folder or isn't an asset.
pub fn resolve(reference: &str, dir: &str) -> Option<String> {
    let cut = reference.find(['?', '#']).map_or(reference, |i| &reference[..i]);
    if cut.is_empty() || cut.starts_with("//") || has_scheme(cut) {
        return None;
    }
    let decoded = percent_decode(cut)?;
    if !ASSET_EXTENSIONS.contains(&extension(decoded.trim_end_matches('/')).as_str()) {
        return None;
    }
    let mut stack: Vec<&str> = if decoded.starts_with('/') { vec![] } else { dir.split('/').filter(|p| !p.is_empty()).collect() };
    for part in decoded.split('/').filter(|p| !p.is_empty()) {
        match part {
            "." => {}
            // Above the folder: never.
            ".." => {
                stack.pop()?;
            }
            _ => stack.push(part),
        }
    }
    if stack.is_empty() {
        return None;
    }
    let joined = stack.join("/");
    safe_parts(&joined)?;
    Some(joined)
}

/// `^[A-Za-z][A-Za-z0-9+.-]*:`
fn has_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-') {
            return false;
        }
    }
    false
}

// ---------------------------------------------------------------- reading what a file names
//
// These run on whatever a page or stylesheet says, so every one is a single
// pass: the Swift app's first regexes backtracked (`url(` and 16 KB of spaces
// took 15 seconds, and `<script` over and over was quadratic). Each search
// below that starts again from a later place remembers its last answer
// (`Seek`), so a run is walked once however many searches start in it.

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Whether `text[at..]` starts with `word`, ASCII case folded.
fn starts_ci(text: &[u8], at: usize, word: &[u8]) -> bool {
    text.len() >= at + word.len() && text[at..at + word.len()].eq_ignore_ascii_case(word)
}

/// The first index at or after `from` whose byte passes `test`, else the
/// length, for searches whose `from` only moves forward.
struct Seek<F: Fn(u8) -> bool> {
    test: F,
    from: usize,
    found: Option<usize>,
}

impl<F: Fn(u8) -> bool> Seek<F> {
    fn new(test: F) -> Self {
        Seek { test, from: 0, found: None }
    }

    fn first(&mut self, text: &[u8], start: usize) -> usize {
        if let Some(found) = self.found {
            if start >= self.from && start <= found {
                return found;
            }
        }
        let mut i = start;
        while i < text.len() && !(self.test)(text[i]) {
            i += 1;
        }
        self.from = start;
        self.found = Some(i);
        i
    }
}

/// Every place `word` starts (ASCII case folded), in order.
fn find_all_ci<'a>(text: &'a [u8], word: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let first = word[0].to_ascii_lowercase();
    (0..text.len()).filter(move |&i| text[i].to_ascii_lowercase() == first && starts_ci(text, i, word))
}

/// `text` with what's between each `<script ...>` and the `</script>` after
/// it taken out: the tag runs to its first `>`, and a script with no
/// `</script>` after its tag keeps its contents.
pub fn without_script_contents(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some(open) = (at..bytes.len()).find(|&i| bytes[i] == b'<' && starts_ci(bytes, i, b"<script") && bytes.get(i + 7).is_none_or(|b| !is_word(*b))) {
        // No `>`: none for a later tag either.
        let Some(gt) = (open + 7..bytes.len()).find(|&i| bytes[i] == b'>') else { break };
        // The `</script>` after the tag.
        let mut close = None;
        let mut i = gt + 1;
        while i < bytes.len() {
            // An end tag as the browser reads one: `</script` then a space,
            // a `/` or `>`, running to the next `>` (`</script/>` and
            // `</script foo>` close it too: Grok's review, 13 Nov).
            if bytes[i] == b'<' && starts_ci(bytes, i, b"</script") && matches!(bytes.get(i + 8), Some(b) if is_space(*b) || *b == b'/' || *b == b'>') {
                if let Some(gt) = (i + 8..bytes.len()).find(|&j| bytes[j] == b'>') {
                    close = Some((i, gt + 1));
                }
                break;
            }
            i += 1;
        }
        let Some((start, end)) = close else { break };
        out.push_str(&text[copied..=gt]);
        copied = start;
        at = end;
    }
    if copied == 0 {
        return text.to_owned();
    }
    out.push_str(&text[copied..]);
    out
}

/// `value` with spaces trimmed, kept in `out` if it's a name a page could
/// read: not empty, and no longer than `MAX_REFERENCE_SOURCE_BYTES`. A longer one
/// is skipped before it's copied.
fn keep(out: &mut Vec<String>, value: &[u8]) {
    let value = value.trim_ascii();
    if !value.is_empty() && value.len() <= MAX_REFERENCE_SOURCE_BYTES {
        out.push(String::from_utf8_lossy(value).trim().to_owned());
    }
}

/// What a page's markup names: `src`, `href`, `poster` and `data`
/// attributes, each part of a `srcset`, and `url()` and `@import` in its
/// style attributes and `<style>` blocks. Scripts' contents are taken out
/// first, so nothing a script says counts. At most `limit`, shared by all of
/// them. The tags are read once, each value as a browser reads it
/// (`html_tags`), so `srcset=` inside a `src`'s value is that value, not
/// another attribute that reads the rest of the page again (Daybreak 118).
pub fn html_references(text: &str, limit: usize) -> Vec<String> {
    let mut out = vec![];
    if limit == 0 {
        return out;
    }
    let markup = without_script_contents(text);
    crate::html_tags::each_attribute(markup.as_bytes(), |_, name, value| {
        if name.eq_ignore_ascii_case(b"srcset") {
            for piece in value.split(|&b| b == b',') {
                if out.len() >= limit {
                    return false;
                }
                if let Some(first) = piece.split(|b| b.is_ascii_whitespace()).find(|s| !s.is_empty()) {
                    keep(&mut out, first);
                }
            }
        } else if [&b"src"[..], b"href", b"poster", b"data"].iter().any(|n| name.eq_ignore_ascii_case(n)) {
            keep(&mut out, value);
        }
        out.len() < limit
    });
    let left = limit.saturating_sub(out.len());
    out.extend(css_references(&markup, left));
    out
}

/// What a stylesheet names: `url()` and `@import`, at most `limit`.
pub fn css_references(text: &str, limit: usize) -> Vec<String> {
    let mut out = css_urls(text, limit);
    let bytes = text.as_bytes();
    let mut next_dq = Seek::new(|b| b == b'"');
    let mut next_sq = Seek::new(|b| b == b'\'');
    for at in find_all_ci(bytes, b"@import") {
        if out.len() >= limit {
            break;
        }
        let mut i = at + 7;
        let spaced = i < bytes.len() && is_space(bytes[i]);
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if !spaced {
            continue;
        }
        let end = match bytes.get(i) {
            Some(b'"') => next_dq.first(bytes, i + 1),
            Some(b'\'') => next_sq.first(bytes, i + 1),
            _ => continue,
        };
        if end < bytes.len() {
            keep(&mut out, &bytes[i + 1..end]);
        }
    }
    out
}

/// Each `url( "..." )`, `url('...')` or `url(bare)`'s value, trimmed.
fn css_urls(text: &str, limit: usize) -> Vec<String> {
    let mut out = vec![];
    if limit == 0 {
        return out;
    }
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut after_open = Seek::new(|b| !is_space(b));
    let mut after_bare = Seek::new(|b| !is_space(b));
    let mut after_dq = Seek::new(|b| !is_space(b));
    let mut after_sq = Seek::new(|b| !is_space(b));
    let mut bare_end = Seek::new(|b| b == b')' || b == b'"' || b == b'\'' || is_space(b));
    let mut next_dq = Seek::new(|b| b == b'"');
    let mut next_sq = Seek::new(|b| b == b'\'');
    let mut resume = 0;
    for open in find_all_ci(bytes, b"url(") {
        if open < resume {
            continue;
        }
        let p = after_open.first(bytes, open + 4);
        let mut found = None;
        if p < n && (bytes[p] == b'"' || bytes[p] == b'\'') {
            let q = if bytes[p] == b'"' { next_dq.first(bytes, p + 1) } else { next_sq.first(bytes, p + 1) };
            if q < n {
                let r = if bytes[p] == b'"' { after_dq.first(bytes, q + 1) } else { after_sq.first(bytes, q + 1) };
                if r < n && bytes[r] == b')' {
                    found = Some((p + 1, q, r));
                }
            }
        } else {
            let e = bare_end.first(bytes, p);
            let r = after_bare.first(bytes, e);
            if r < n && bytes[r] == b')' {
                found = Some((p, e, r));
            }
        }
        let Some((start, end, close)) = found else { continue };
        keep(&mut out, &bytes[start..end]);
        resume = close + 1;
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Whether a page has scripts of its own, which the preview runs only once
/// Mike says so for those exact bytes: the pane asks rather than showing a
/// page that draws itself as blank boxes. A `<script`, an `on...=` handler in
/// a tag or a `javascript:` link anywhere counts, even in a comment: asking
/// wrongly costs a bar, missing one looks broken.
pub fn has_scripts(html: &[u8]) -> bool {
    let text = &html[..html.len().min(SCRIPT_SCAN_BYTES)];
    if find_all_ci(text, b"<script").next().is_some() || find_all_ci(text, b"javascript:").next().is_some() {
        return true;
    }
    // A handler: a space, `on`, letters, spaces, `=`, where the last `<`
    // before it comes after the last `>`.
    let (mut last_open, mut last_close): (Option<usize>, Option<usize>) = (None, None);
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if b == b'<' {
            last_open = Some(i);
        } else if b == b'>' {
            last_close = Some(i);
        } else if is_space(b) && starts_ci(text, i + 1, b"on") {
            let mut j = i + 3;
            let letters = j;
            while j < text.len() && text[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j > letters {
                while j < text.len() && is_space(text[j]) {
                    j += 1;
                }
                let in_tag = last_open.is_some_and(|o| last_close.is_none_or(|c| o > c));
                if text.get(j) == Some(&b'=') && in_tag {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// SHA-256 of a page's bytes, hex: what a consent to run scripts is for.
pub fn digest(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// A page file read for the bar over it: whether it has scripts, and the
/// digest of what was read. None for anything but a regular file up to
/// `MAX_BYTES`, opened without blocking so a FIFO named `.html` can't hang.
pub fn scan(path: &Path) -> Option<(bool, String)> {
    use std::io::Read as _;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // The real path the gate found: a link swapped in since is refused,
        // so what's scanned is what `serve` will read.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options.open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut data = vec![];
    file.take(MAX_BYTES + 1).read_to_end(&mut data).ok()?;
    if data.len() as u64 > MAX_BYTES {
        return None;
    }
    Some((has_scripts(&data), digest(&data)))
}

/// Which local pages Mike has let run their scripts: one digest per file, so a
/// page an agent rewrites has to be allowed again (`PreviewScriptConsent`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScriptConsent(pub Vec<(String, String)>);

impl ScriptConsent {
    /// Plenty for the pages someone looks at; the oldest go first.
    pub const LIMIT: usize = 200;

    pub fn allows(&self, path: &str, digest: &str) -> bool {
        self.0.iter().any(|(p, d)| p == path && d == digest)
    }

    pub fn allow(&mut self, path: &str, digest: &str) {
        self.0.retain(|(p, _)| p != path);
        self.0.push((path.to_owned(), digest.to_owned()));
        let over = self.0.len().saturating_sub(Self::LIMIT);
        self.0.drain(..over);
    }
}

#[cfg(test)]
mod tests;
