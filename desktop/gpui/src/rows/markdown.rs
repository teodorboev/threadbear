// Reply text: MarkdownRenderer.swift's look in gpui-kit's TextView. The kit
// does the blocks; this sets its sizes, spacing and colours to the Swift
// app's, and sends links where they should go.
use super::metrics::CODE_PAD;
use crate::open_gate::Origin;
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::base::{TextViewState, TextViewStyle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;

/// How far a list's text sits in from the text around it (listIndent).
const LIST_INDENT: f32 = 30.;
/// How far short of a list item's text its marker ends: the Swift bullet
/// (6.8 wide at 15 pt) starts 18 before the text, so ends 11.2 before it.
const MARKER_END: f32 = 11.2;
/// The gap between paragraphs (paragraphGap).
const PARAGRAPH_GAP: f32 = 10.;

/// The reading column's style. Built once per appearance and shared: a new
/// `heading` closure per frame would make the kit see a different style and
/// lay every reply out again.
pub(crate) fn reading_style(cx: &App) -> TextViewStyle {
    let (label, muted, link, code_bg, inline_bg, border, hairline, mono) = (
        style::label_text(cx),
        style::secondary_label(cx),
        style::link_text(cx),
        style::code_background(cx),
        style::inline_code_background(cx),
        style::card_border(cx),
        style::hairline(cx),
        cx.theme().mono_font_family.clone(),
    );
    let rem = f32::from(cx.theme().font_size);
    let code_block = StyleRefinement::default().rounded(px(7.)).border_0().p(px(CODE_PAD)).text_size(px(size::CODE)).line_height(px(size::CODE_LINE)).font_family(mono.clone());
    // The pill behind inline code, with a little room either side (pillPad);
    // the kit reads padding and radius from this.
    let inline_code = StyleRefinement::default().bg(inline_bg).rounded(px(4.)).px(px(2.5)).py(px(1.)).text_size(px(size::CODE)).font_family(mono.clone()).text_color(label);
    // A rule under the header and hairlines between rows, none around them
    // (renderTable). No cell backgrounds.
    let table = StyleRefinement::default().border_0().rounded(px(0.));
    let table_head = StyleRefinement::default().text_size(px(size::LABEL)).font_weight(FontWeight::SEMIBOLD).text_color(muted).bg(gpui_kit::transparent_black()).border_color(border);
    // Transparent: the kit draws a column divider in the cell border colour, and
    // the rows carry the rules (renderTable has no vertical lines).
    let table_cell = StyleRefinement::default().py(px(4.)).px(px(12.)).border_color(gpui_kit::transparent_black());
    TextViewStyle::default()
        .with_foreground(label)
        .with_muted_foreground(muted)
        .with_link(link)
        // Swift's text views draw a link in the link colour alone, with no
        // line under it, even on hover (TextKit's linkTextAttributes).
        .with_link_underline(false)
        .with_selection(cx.theme().selection)
        .with_code_background(code_bg)
        .with_border(hairline)
        .with_paragraph_gap(rems(PARAGRAPH_GAP / rem))
        .with_list_item_gap(rems(style::zoomed(6.) / rem))
        .with_table_natural_width(true)
        .with_heading(move |level| {
            // Sizes and the 6 above / 6 below from MarkdownRenderer (the
            // paragraph before leaves 10 of the 16 above a heading).
            let (font, line) = match level {
                1 => (18., 26.),
                2 => (16., 23.),
                _ => (15., 21.),
            };
            StyleRefinement::default().text_size(px(font)).line_height(px(line)).font_weight(FontWeight::SEMIBOLD).pt(px(6.)).pb(px(6.))
        })
        .with_code_block(code_block)
        .with_inline_code(HighlightStyle { background_color: Some(inline_bg), ..Default::default() })
        .with_inline_code_style(inline_code)
        // The kit right-aligns a marker in the indent. Swift starts a bullet
        // 18 before the text and a number 22 before (markerStart): ending
        // them 11.2 short of the text puts the bullet and "1." where the
        // Swift ones are (a two-figure number sits a little further out).
        .with_list_style(StyleRefinement::default().text_color(label).min_w(px(LIST_INDENT)).pr(px(MARKER_END)))
        .with_blockquote_style(StyleRefinement::default().border_l(px(0.)).pl(px(14.)).text_color(muted))
        .with_table(table)
        .with_table_head(table_head)
        .with_table_cell(table_cell)
        .with_dark(cx.theme().is_dark())
}

/// Whether markdown text opens with a heading ("# " to "###### ").
pub(crate) fn starts_with_heading(text: &str) -> bool {
    let text = text.trim_start_matches([' ', '\n']);
    let hashes = text.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && text[hashes..].starts_with([' ', '\t'])
}

/// `WorkLinks.escapeDestinations` and the "↑" after a work link: a space ends a
/// link's destination for the parser, and agents write `[tests pass](work:npm
/// test)`. The arrow says the link goes up the thread, not out of it, and goes
/// in the link's own words so it's coloured and selected with them.
/// `![alt](url)` in a reply (`MarkdownRenderer`'s images): a picture on this
/// Mac is drawn in the text; a web address, another Mac's file or one that's
/// gone stays a link with its alt text, "[alt]". A web picture is never
/// fetched: what an agent writes can't make the app load an address by
/// itself.
pub(crate) fn pictures(text: &str, local: bool) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if !text.contains("![") {
        return Cow::Borrowed(text);
    }
    let Ok(tree) = markdown::to_mdast(text, &markdown::ParseOptions::gfm()) else { return Cow::Borrowed(text) };
    let mut swaps: Vec<(usize, usize, String)> = Vec::new();
    collect_images(&tree, &mut |image| {
        let Some(at) = &image.position else { return };
        if local && picture_path(&image.url).is_some_and(|path| is_picture_file(&path)) {
            return;
        }
        let alt = if image.alt.is_empty() { image.url.as_str() } else { image.alt.as_str() };
        let words: String = alt.chars().flat_map(|c| if matches!(c, '[' | ']' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
        swaps.push((at.start.offset, at.end.offset, format!("[\\[{words}\\]](<{}>)", image.url.replace('>', "%3E"))));
    });
    if swaps.is_empty() {
        return Cow::Borrowed(text);
    }
    swaps.sort_by_key(|s| s.0);
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for (start, end, link) in swaps {
        if start < at || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(&link);
        at = end;
    }
    out.push_str(&text[at..]);
    Cow::Owned(out)
}

fn collect_images(node: &markdown::mdast::Node, found: &mut impl FnMut(&markdown::mdast::Image)) {
    if let markdown::mdast::Node::Image(image) = node {
        found(image);
    }
    for child in node.children().into_iter().flatten() {
        collect_images(child, found);
    }
}

/// The file a picture's address names on this Mac, if it's a file.
pub(crate) fn picture_path(url: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        windows_picture_path(url)
    }
    #[cfg(not(windows))]
    {
        if let Some(rest) = url.strip_prefix("file://") {
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            return path.starts_with('/').then(|| crate::files::percent_decode(path).into());
        }
        url.starts_with('/').then(|| url.into())
    }
}

/// Only a drive's absolute path, either as written or in a local file URL.
/// A share or a device must be refused here, before the image checks touch it.
#[cfg(any(windows, test))]
fn windows_picture_path(url: &str) -> Option<std::path::PathBuf> {
    use threadbear_core::open_policy::{is_device_name, percent_decode};
    let path = if let Some(rest) = url.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost/").unwrap_or(rest);
        percent_decode(rest.strip_prefix('/').unwrap_or(rest))
    } else {
        url.to_owned()
    };
    let bytes = path.as_bytes();
    let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') && bytes.get(2).is_some_and(|b| matches!(b, b'/' | b'\\'));
    if !drive || path.chars().any(char::is_control) || path[2..].contains(':') || path.split(['/', '\\']).any(is_device_name) {
        return None;
    }
    Some(path.into())
}

/// The biggest picture file a reply draws: the image loader reads a file
/// whole into memory.
const PICTURE_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// A picture the app can draw, there now: a raster picture file
/// (`raster_file`) with a picture's extension.
fn is_picture_file(path: &std::path::Path) -> bool {
    let kind = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let picture = matches!(kind.as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"));
    // Windows' asset loader checks the file and its bytes off the drawing
    // thread. A guard walk per picture per frame would make scrolling stall.
    #[cfg(windows)]
    {
        picture
    }
    #[cfg(not(windows))]
    {
        picture && raster_file(path)
    }
}

/// A regular file (not a device like /dev/zero, a pipe or a folder; a link
/// is followed to what it names) of at most PICTURE_MAX_BYTES that starts
/// as a PNG, JPEG, GIF, WebP, BMP or TIFF does. gpui's loader reads a path
/// whole, and what it can't place as one of those it reads as an SVG, whose
/// `<image href>` it then reads from the disk too, /dev/zero included
/// (Codex's second look at Daybreak 95). Remembered by the file's size and
/// time, as rows ask on every frame.
fn raster_file(path: &std::path::Path) -> bool {
    use std::sync::{LazyLock, Mutex};
    static SEEN: LazyLock<Mutex<std::collections::HashMap<(std::path::PathBuf, u64, Option<std::time::SystemTime>), bool>>> = LazyLock::new(Default::default);
    let Ok(meta) = threadbear_core::open_policy::local_metadata(path) else { return false };
    if !meta.is_file() || meta.len() > PICTURE_MAX_BYTES {
        return false;
    }
    let key = (path.to_owned(), meta.len(), meta.modified().ok());
    if let Some(known) = SEEN.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return *known;
    }
    let raster = threadbear_core::open_policy::local_read(path, 12).is_ok_and(|head| raster_signature(&head));
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.len() > 2000 {
        seen.clear();
    }
    seen.insert(key, raster);
    raster
}

/// The first bytes of the raster formats the app draws.
fn raster_signature(head: &[u8]) -> bool {
    head.starts_with(b"\x89PNG\r\n\x1a\n")
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || (head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP")
        || head.starts_with(b"BM")
        || head.starts_with(b"II*\0")
        || head.starts_with(b"MM\0*")
}

/// The text view's pictures (`linked_text` gives every row's text view
/// this, a reply's, an agent's message's and a plan's alike). `local` is
/// whether the thread is this runner's: another Mac's draws no file from
/// this one. The checks `pictures` made are made again here, as this is
/// what the image loader reads, and text that never went through
/// `pictures` (an agent's message, a plan, a chip's link) reaches it too
/// (Daybreak 95 and 102, 11 Nov 2026). Anything else draws nothing.
/// A picture a transcript names, as the image loader may be handed it: the
/// path when it's a raster picture file (`raster_file`), and an empty
/// path (which draws nothing) for anything else. gpui's loader reads a path
/// whole, and a review's shots, a tool's pictures or a message's can be
/// another runner's word in a moved thread, naming /dev/zero (Codex's review
/// of Daybreak 95). For every `img` of a path from a thread.
pub(crate) fn picture_file(path: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(path);
    if raster_file(&path) {
        path
    } else {
        std::path::PathBuf::new()
    }
}

pub(crate) fn picture_source(uri: &gpui_kit::SharedUri, local: bool) -> gpui_kit::ImageSource {
    #[cfg(windows)]
    {
        return match picture_path(uri.as_ref()).filter(|_| local) {
            Some(path) => file_picture(&path.to_string_lossy()),
            None => ImageSource::Resource(Resource::Path(std::sync::Arc::from(std::path::Path::new("")))),
        };
    }
    #[cfg(not(windows))]
    {
        let path = picture_path(uri.as_ref()).filter(|path| local && is_picture_file(path)).map(|path| picture_file(&path.to_string_lossy())).unwrap_or_default();
        gpui_kit::ImageSource::Resource(gpui_kit::Resource::Path(std::sync::Arc::from(path.as_path())))
    }
}

/// Windows decodes a snapshot read under the path guard. GPUI's default
/// loader would open the path again later, after a link could be swapped in.
/// Its asset cache keeps this work off the drawing thread and out of frames.
pub(crate) fn file_picture(path: &str) -> ImageSource {
    #[cfg(windows)]
    {
        let path = std::path::PathBuf::from(path);
        ImageSource::Custom(std::sync::Arc::new(move |window, cx| {
            let result = window.use_asset::<LocalPicture>(&path, cx);
            if result.as_ref().is_some_and(|r| r.is_ok()) {
                cx.default_global::<PictureRetries>().attempts.remove(&path);
            }
            if result.as_ref().is_some_and(|r| r.is_err()) && cx.default_global::<PictureRetries>().pending.insert(path.clone()) {
                let retries = cx.default_global::<PictureRetries>();
                if retries.attempts.len() > 2000 {
                    retries.attempts.clear();
                }
                let attempt = retries.attempts.entry(path.clone()).or_default();
                let delay = 1u64 << (*attempt).min(6);
                *attempt = attempt.saturating_add(1);
                let path = path.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_secs(delay)).await;
                    let _ = cx.update(|cx| {
                        cx.remove_asset::<LocalPicture>(&path);
                        cx.default_global::<PictureRetries>().pending.remove(&path);
                        cx.refresh_windows();
                    });
                })
                .detach();
            }
            result
        }))
    }
    #[cfg(not(windows))]
    {
        picture_file(path).into()
    }
}

#[cfg(windows)]
struct LocalPicture;

#[cfg(windows)]
#[derive(Default)]
struct PictureRetries {
    pending: std::collections::HashSet<std::path::PathBuf>,
    attempts: std::collections::HashMap<std::path::PathBuf, u32>,
}
#[cfg(windows)]
impl Global for PictureRetries {}

#[cfg(windows)]
fn checked_picture_bytes(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut file = threadbear_core::open_policy::local_open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > PICTURE_MAX_BYTES {
        return Err(std::io::Error::other("Not a picture file"));
    }
    let mut bytes = Vec::with_capacity(12);
    (&mut file).take(12).read_to_end(&mut bytes)?;
    if !raster_signature(&bytes) {
        return Err(std::io::Error::other("Not a raster picture"));
    }
    file.take(PICTURE_MAX_BYTES + 1 - bytes.len() as u64).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > PICTURE_MAX_BYTES {
        return Err(std::io::Error::other("Picture is too big"));
    }
    Ok(bytes)
}

#[cfg(windows)]
impl Asset for LocalPicture {
    type Source = std::path::PathBuf;
    type Output = Result<std::sync::Arc<RenderImage>, ImageCacheError>;

    fn load(path: Self::Source, cx: &mut App) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let bytes = checked_picture_bytes(&path)?;
            let format = crate::viewer::format_of(&path.to_string_lossy(), &bytes).ok_or_else(|| std::io::Error::other("Not a picture"))?;
            Image::from_bytes(format, bytes).to_image_data(renderer).map_err(Into::into)
        }
    }
}

/// A reply's text in `thread`'s transcript, ready to parse: its buttons
/// that aren't `thread`'s own as words (`unknown_as_words`).
pub(crate) fn prepare(text: &str, thread: &str) -> String {
    link_code_paths(&escape_work_links(&crate::reply_buttons::unknown_as_words(text, thread)))
}

fn escape_work_links(text: &str) -> String {
    if !text.contains("](work:") && !text.contains("](thread:") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    loop {
        let (start, is_work) = match (rest.find("](work:"), rest.find("](thread:")) {
            (Some(w), Some(t)) if w < t => (w, true),
            (_, Some(t)) => (t, false),
            (Some(w), None) => (w, true),
            (None, None) => {
                out.push_str(rest);
                return out;
            }
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        // No `)` to end it: not a link, and the rest of its line is used up.
        let end = after.find([')', '\n']).unwrap_or(after.len());
        if after[end..].starts_with(')') {
            if is_work {
                out.push('\u{2009}');
                out.push('↑');
            }
            out.push_str("](");
            out.push_str(&after[..end].replace(' ', "%20"));
            out.push(')');
            rest = &after[end + 1..];
        } else {
            out.push_str("](");
            out.push_str(&after[..end]);
            rest = &after[end..];
        }
    }
}

/// `` `path` `` written in reply words: a code span that reads as a file path
/// (`FilePathRefs.looksLikePath`) becomes a link to it, kept looking like code.
/// Only single-backtick spans on one line outside fenced code count, as cmark
/// would find them; the kit draws the pill and the link colour.
fn link_code_paths(text: &str) -> String {
    if !text.contains('`') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 32);
    let mut in_fence = false;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start_matches(' ');
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        link_spans_in_line(line, &mut out);
    }
    out
}

fn link_spans_in_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            // Copy up to the next backtick.
            let next = line[i..].find('`').map(|n| i + n).unwrap_or(line.len());
            out.push_str(&line[i..next]);
            i = next;
            continue;
        }
        // A run of backticks opens a span closed by a run of the same length.
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let search_from = i + run;
        let close = line[search_from..].find(&"`".repeat(run)).map(|n| search_from + n);
        let Some(close) = close.filter(|c| *c >= search_from) else {
            out.push_str(&line[i..i + run]);
            i += run;
            continue;
        };
        let end = close + run;
        let span = &line[i..end];
        let code = &line[search_from..close];
        // Already a link's words (`[` before it, `](` after it), or more than
        // one backtick around it: left as it is.
        let in_link = i > 0 && bytes[i - 1] == b'[' && line[end..].starts_with("](");
        if run == 1 && !in_link && looks_like_path(code) {
            out.push('[');
            out.push_str(span);
            out.push_str("](tbpath:?p=");
            out.push_str(&encode_component(code));
            out.push(')');
        } else {
            out.push_str(span);
        }
        i = end;
    }
}

/// `FilePathRefs.looksLikePath`: no scheme, and either rooted (`/`, `~/`, `./`,
/// `../`) or ending in a file name with an extension; folders need the leading
/// root or a slash in them; single spaces only where they can't be a command.
pub(crate) fn looks_like_path(text: &str) -> bool {
    if text.is_empty() || text.chars().count() >= 400 || text.contains("://") || text.contains('*') || text.contains('<') || text.contains('`') {
        return false;
    }
    let path = strip_line(split_query(text).0);
    if text.chars().any(char::is_whitespace) && !spaced_path_ok(path) {
        return false;
    }
    if path.starts_with('/') {
        return path.chars().count() > 1;
    }
    if path.starts_with("~/") || path.starts_with("./") || path.starts_with("../") {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let good_ext = !ext.is_empty() && ext.chars().count() <= 10 && ext.chars().all(char::is_alphanumeric) && ext.chars().any(char::is_alphabetic) && name.chars().count() > ext.chars().count() + 1;
    if !good_ext {
        // `runner/src/` and the like: a folder inside the project.
        return path.ends_with('/') && path[..path.len() - 1].contains('/');
    }
    !(!path.contains('/') && DOMAIN_ENDINGS.contains(&ext.to_lowercase().as_str()))
}

/// Endings that are web addresses far more often than files.
const DOMAIN_ENDINGS: [&str; 9] = ["com", "org", "net", "io", "dev", "co", "ai", "au", "uk"];

/// `spacedPathOK`: single spaces only, after the first folder, no flags or
/// shell punctuation, and a spaced last part needs an extension.
fn spaced_path_ok(path: &str) -> bool {
    if !path.contains(' ') || path.chars().any(|c| c.is_whitespace() && c != ' ') || path.starts_with(' ') || path.ends_with(' ') || path.contains("  ") {
        return false;
    }
    if path.chars().any(|c| matches!(c, '|' | ';' | '$' | '>' | '"' | '\'')) || path.contains("&&") {
        return false;
    }
    let (Some(slash), Some(space)) = (path.find('/'), path.find(' ')) else { return false };
    if slash > space || path.split(' ').any(|w| w.starts_with('-')) {
        return false;
    }
    let last = if path.ends_with('/') { "" } else { path.rsplit('/').next().unwrap_or("") };
    !last.contains(' ') || last.rsplit_once('.').is_some()
}

/// `Foo.swift:12` or `Foo.swift:12:4` names the file, and a line in it.
fn strip_line(text: &str) -> &str {
    let mut path = text;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, tail)) if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => path = head,
            _ => break,
        }
    }
    path
}

/// A file and any `?query` or `#fragment` for its page, split at the first.
fn split_query(text: &str) -> (&str, Option<&str>) {
    match text.find(['?', '#']) {
        Some(mark) => (&text[..mark], Some(&text[mark..])),
        None => (text, None),
    }
}

/// Everything that isn't safe in a link's destination, percent-escaped.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `%20` and friends back to what the link's words were.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl ThreadPane {
    /// The kit's document for a row, kept so scrolling a row away and back
    /// doesn't parse it again, and so a streaming reply only parses what's new
    /// (`set_text` appends when the text extends what's there).
    pub(crate) fn markdown_state(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> Entity<TextViewState> {
        let state = match self.rows.documents.get(key) {
            Some(state) => state.clone(),
            None => {
                let state = cx.new(|cx| TextViewState::markdown(text, cx));
                if self.rows.documents.len() >= DOCUMENTS_KEPT {
                    // Far more than a screen's worth; the oldest are the least
                    // likely to come back soon.
                    if let Some(oldest) = self.rows.document_order.first().cloned() {
                        self.rows.documents.remove(&oldest);
                        self.rows.document_order.remove(0);
                    }
                }
                self.rows.document_order.push(key.to_owned());
                self.rows.documents.insert(key.to_owned(), state.clone());
                state
            }
        };
        state.update(cx, |s, cx| s.set_text(text, cx));
        state
    }

    /// Where a link goes when it's clicked with no key held (a ⌘-click comes
    /// through `linked_text`): reply_links.rs's `follow_link`, as it says
    /// what each kind of link does.
    pub(crate) fn link_clicked(&mut self, url: &str, row_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.follow_link(url, row_id, false, window, cx);
    }

    pub(crate) fn origin(&self) -> Origin {
        if self.summary.is_remote() {
            Origin::OtherMac
        } else {
            Origin::ThisMac
        }
    }
}

/// How many row documents a thread keeps parsed.
const DOCUMENTS_KEPT: usize = 600;

#[cfg(test)]
mod tests {
    use super::{looks_like_path, pictures, prepare};

    #[test]
    fn windows_pictures_name_a_drive_file_before_any_file_access() {
        for (url, path) in [
            (r"C:\pictures\shot.png", r"C:\pictures\shot.png"),
            ("c:/pictures/shot.png", "c:/pictures/shot.png"),
            ("file:///C:/pictures/a%20b.png", "C:/pictures/a b.png"),
            ("file://localhost/C:/pictures/shot.png", "C:/pictures/shot.png"),
            (r"file://C:\pictures\shot.png", r"C:\pictures\shot.png"),
        ] {
            assert_eq!(super::windows_picture_path(url), Some(std::path::PathBuf::from(path)), "{url}");
        }
        for url in [
            r"\\host\share\shot.png",
            "//host/share/shot.png",
            "file://host/share/shot.png",
            "file:////host/share/shot.png",
            "file:///%5C%5Chost/share/shot.png",
            r"\\?\C:\pictures\shot.png",
            r"\\.\pipe\shot.png",
            r"\??\C:\shot.png",
            r"C:\pictures\NUL.tar.png",
            r"C:\COM¹\shot.png",
            r"C:\pictures\NUL:shot.png",
            "C:/pictures/file:stream.png",
            "C:shot.png",
            "/pictures/shot.png",
            "shot.png",
            "file:///C:/pictures/a%00b.png",
            "https://x.y/shot.png",
            "",
        ] {
            assert_eq!(super::windows_picture_path(url), None, "{url}");
        }
    }

    #[test]
    fn a_web_picture_or_another_macs_stays_a_link() {
        assert_eq!(pictures("see ![chart](https://x.y/c.png)", true), "see [\\[chart\\]](<https://x.y/c.png>)");
        assert_eq!(pictures("![](/nowhere/gone.png)", true), "[\\[/nowhere/gone.png\\]](</nowhere/gone.png>)", "a file that's gone");
        assert_eq!(pictures("no pictures here", true), "no pictures here");
        assert_eq!(pictures("`![x](https://a.b)` in code", true), "`![x](https://a.b)` in code", "code is left as written");
        let dir = std::env::temp_dir().join(format!("tb-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shot = dir.join("shot.png");
        std::fs::write(&shot, b"\x89PNG\r\n\x1a\n, a png's first bytes").unwrap();
        let reply = format!("![shot]({})", shot.display());
        assert_eq!(pictures(&reply, true), reply, "a picture on this Mac is drawn");
        assert!(pictures(&reply, false).starts_with("[\\[shot\\]]"), "another Mac's thread draws none from here");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the image loader is handed for a picture's address: the file,
    /// or the empty path that draws nothing.
    fn source_path(url: &str, local: bool) -> String {
        match super::picture_source(&url.to_owned().into(), local) {
            gpui_kit::ImageSource::Resource(gpui_kit::Resource::Path(path)) => path.to_string_lossy().into_owned(),
            #[cfg(windows)]
            gpui_kit::ImageSource::Custom(_) => super::picture_path(url).filter(|p| super::checked_picture_bytes(p).is_ok()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => panic!("a path"),
        }
    }

    /// Daybreak 95: an agent's message never went through `pictures`, so
    /// the image loader's own source checks the thread and the file.
    #[test]
    fn the_image_loader_only_gets_a_picture_file_on_this_macs_thread() {
        let dir = std::env::temp_dir().join(format!("tb-picture-source-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shot = dir.join("shot.png");
        std::fs::write(&shot, b"\x89PNG\r\n\x1a\n and the rest").unwrap();
        let shown = shot.to_string_lossy().into_owned();
        assert_eq!(source_path(&shown, true), shown, "a picture on this Mac's thread");
        assert_eq!(source_path(&["file://", shown.as_str()].concat(), true), shown);
        assert_eq!(source_path(&shown, false), "", "another Mac's thread names no file here");
        for url in ["file:///dev/zero", "/dev/zero", "/dev/urandom", "file:///etc/passwd", "/tmp", "https://x.y/c.png"] {
            assert_eq!(source_path(url, true), "", "{url}");
        }
        // A link to a device with a picture's name, a folder with one, and
        // a file too big to read whole.
        let device = dir.join("zero.png");
        let _ = std::fs::remove_file(&device);
        // Windows has no /dev/zero to link to (and this didn't compile
        // there, which stopped the Windows job's tests), so there the path
        // leads nowhere, which is refused too.
        #[cfg(unix)]
        std::os::unix::fs::symlink("/dev/zero", &device).unwrap();
        assert_eq!(source_path(&device.to_string_lossy(), true), "");
        let folder = dir.join("folder.png");
        std::fs::create_dir_all(&folder).unwrap();
        assert_eq!(source_path(&folder.to_string_lossy(), true), "");
        let big = dir.join("big.png");
        std::fs::File::create(&big).unwrap().set_len(super::PICTURE_MAX_BYTES + 1).unwrap();
        assert_eq!(source_path(&big.to_string_lossy(), true), "");
        // A path from a thread for any other picture (a review's shot, a
        // tool's picture): only a regular file of a sane size.
        assert_eq!(super::picture_file(&shown), std::path::PathBuf::from(&shown));
        for path in ["/dev/zero", "/tmp", ""] {
            assert_eq!(super::picture_file(path), std::path::PathBuf::new(), "{path}");
        }
        for path in [&device, &folder, &big] {
            assert_eq!(super::picture_file(&path.to_string_lossy()), std::path::PathBuf::new(), "{path:?}");
        }
        // An SVG named as a picture, whose <image> would have the loader
        // read /dev/zero, and text: neither starts as a picture does.
        let svg = dir.join("payload.png");
        std::fs::write(&svg, br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1" height="1"><image width="1" height="1" xlink:href="/dev/zero"/></svg>"#)
            .unwrap();
        let text = dir.join("notes.jpg");
        std::fs::write(&text, b"hello").unwrap();
        for path in [&svg, &text] {
            assert_eq!(super::picture_file(&path.to_string_lossy()), std::path::PathBuf::new(), "{path:?}");
            assert_eq!(source_path(&path.to_string_lossy(), true), "", "{path:?}");
        }
        for head in [&b"\xFF\xD8\xFF\xE0"[..], b"GIF89a..", b"RIFF\0\0\0\0WEBPVP8 ", b"BM....", b"II*\0", b"MM\0*"] {
            assert!(super::raster_signature(head), "{head:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_paths_as_paths_and_commands_as_commands() {
        for path in ["app/Sources/Foo.swift", "Foo.swift:12", "/tmp/a.txt:12:4", "~/.zshrc", "./check.sh", "runner/src/", "guide.md#links"] {
            assert!(looks_like_path(path), "{path} should read as a path");
        }
        for not_path in ["npm test", "tb land", "example.com", "readme", "-rf", "a*b", "https://x.y/z", "open exports/a.mp4"] {
            assert!(!looks_like_path(not_path), "{not_path} shouldn't read as a path");
        }
        // Spaces only where they can't be a command.
        assert!(looks_like_path("exports/Threadbear Launch.mp4"));
        assert!(!looks_like_path("./check.sh --app"));
    }

    #[test]
    fn code_paths_become_links_outside_fences_and_links() {
        assert_eq!(prepare("see `src/a.rs` here", "t"), "see [`src/a.rs`](tbpath:?p=src/a.rs) here");
        assert_eq!(prepare("`npm test` stays", "t"), "`npm test` stays");
        // Already a link's words, or in a code block: left as they are.
        assert_eq!(prepare("[`src/a.rs`](https://x.y)", "t"), "[`src/a.rs`](https://x.y)");
        assert_eq!(prepare("```\n`src/a.rs`\n```", "t"), "```\n`src/a.rs`\n```");
        // Work links keep their words, gain the arrow and escape their spaces.
        assert_eq!(prepare("[tests](work:npm test)", "t"), "[tests\u{2009}↑](work:npm%20test)");
    }
}
