//! One pass over HTML's start tags, for the two places the app reads markup
//! it didn't write: the local preview's list of what a page names
//! (local_preview.rs) and a browser drag's `<img>` (the composer's
//! draggingEntered, native/composer_media.m).
//!
//! Each byte is looked at a bounded number of times and each attribute's
//! value is read once, as a browser's tokenizer reads it, so nothing inside
//! a value is ever taken for another attribute. Both used to search the whole
//! text for `name=` with a regex or a find, and Daybreak found both: a drag's
//! `<img ` over and over took seconds in a backtracking regex on the main
//! thread (finding 120), and a page of `src=srcset=` read the rest of the
//! page again from every `srcset=` inside a value, 4 MiB into 15.6 GiB
//! (finding 118, 13 Nov 2026).

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

/// Calls `visit(tag, name, value)` for each attribute with a value in each
/// start tag, in order, until it returns false. A tag is `<` then a letter;
/// anything else (`<!--`, `</p>`, a `<` in text) is passed over a byte at a
/// time, so what's in a comment is still read, as before. A quote that never
/// closes ends the reading, as the end of the file ends a browser's.
pub fn each_attribute(text: &[u8], mut visit: impl FnMut(&[u8], &[u8], &[u8]) -> bool) {
    let n = text.len();
    let mut i = 0;
    while i < n {
        let Some(lt) = text[i..].iter().position(|&b| b == b'<') else { return };
        i += lt + 1;
        if !text.get(i).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        let tag_start = i;
        while i < n && !is_space(text[i]) && !matches!(text[i], b'/' | b'>') {
            i += 1;
        }
        let tag = &text[tag_start..i];
        // The attributes, up to the tag's `>`.
        loop {
            while i < n && (is_space(text[i]) || text[i] == b'/') {
                i += 1;
            }
            if i >= n {
                return;
            }
            if text[i] == b'>' {
                i += 1;
                break;
            }
            // A name runs to a space, `/`, `>` or `=`, but its first
            // character can be anything, `=` included, as a browser has it.
            let name_start = i;
            i += 1;
            while i < n && !is_space(text[i]) && !matches!(text[i], b'/' | b'>' | b'=') {
                i += 1;
            }
            let name = &text[name_start..i];
            let mut j = i;
            while j < n && is_space(text[j]) {
                j += 1;
            }
            if text.get(j) != Some(&b'=') {
                // No value: what follows is the next attribute.
                i = j;
                continue;
            }
            i = j + 1;
            while i < n && is_space(text[i]) {
                i += 1;
            }
            let value = match text.get(i) {
                None => return,
                Some(&q @ (b'"' | b'\'')) => {
                    let start = i + 1;
                    let Some(len) = text[start..].iter().position(|&b| b == q) else { return };
                    i = start + len + 1;
                    &text[start..start + len]
                }
                // `name=>`: an empty value, and the tag ends.
                Some(b'>') => &text[i..i],
                Some(_) => {
                    let start = i;
                    while i < n && !is_space(text[i]) && text[i] != b'>' {
                        i += 1;
                    }
                    &text[start..i]
                }
            };
            if !visit(tag, name, value) {
                return;
            }
        }
    }
}

/// The most of a drag's HTML that's read for its `<img>`. A browser's image
/// drag carries one tag; more than this isn't an image drag.
pub const DRAG_HTML_BYTES: usize = 256 * 1024;

/// Whether a browser drag's HTML is an `<img>` of `url` itself, so an
/// extensionless URL is fetched as a picture only when the page showed it as
/// one (an ordinary link must never be fetched: a GET might use up a sign-in
/// or unsubscribe link). `&amp;` in the `src` reads as `&`. Anything over
/// `DRAG_HTML_BYTES` is no.
pub fn drag_html_shows_image(html: &[u8], url: &str) -> bool {
    if html.len() > DRAG_HTML_BYTES || url.is_empty() {
        return false;
    }
    let want = url.as_bytes();
    let mut found = false;
    each_attribute(html, |tag, name, value| {
        if tag.eq_ignore_ascii_case(b"img") && name.eq_ignore_ascii_case(b"src") && unescaped_amp_eq(value, want) {
            found = true;
        }
        !found
    });
    found
}

/// Whether `value` with each `&amp;` read as `&` is `want`, without copying.
fn unescaped_amp_eq(value: &[u8], want: &[u8]) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < value.len() {
        if j >= want.len() {
            return false;
        }
        if value[i..].starts_with(b"&amp;") {
            if want[j] != b'&' {
                return false;
            }
            i += 5;
        } else {
            if value[i] != want[j] {
                return false;
            }
            i += 1;
        }
        j += 1;
    }
    j == want.len()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn attributes(text: &str) -> Vec<(String, String, String)> {
        let mut out = vec![];
        each_attribute(text.as_bytes(), |t, n, v| {
            let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
            out.push((s(t), s(n), s(v)));
            true
        });
        out
    }

    #[test]
    fn reads_each_value_once() {
        let a = |t: &str, n: &str, v: &str| (t.to_owned(), n.to_owned(), v.to_owned());
        assert_eq!(attributes("<img src=srcset=one.png>"), [a("img", "src", "srcset=one.png")], "srcset= inside a value is the value");
        assert_eq!(attributes("<img alt=\"src=x.png\" src = 'y.png' hidden data=z.png/>"), [a("img", "alt", "src=x.png"), a("img", "src", "y.png"), a("img", "data", "z.png/")]);
        assert_eq!(attributes("<p>src=x.png</p><a\nhref=b.css>"), [a("a", "href", "b.css")], "text between tags names nothing");
        assert_eq!(attributes("<img src=\"never closed"), [], "a quote that never closes ends it");
        assert_eq!(attributes("<a b=>c</a><img/src=d.png>"), [a("a", "b", ""), a("img", "src", "d.png")]);
    }

    #[test]
    fn a_drag_shows_its_image_only_in_an_img_src() {
        let url = "https://cdn.example.com/p?id=1&size=2";
        assert!(drag_html_shows_image(b"<meta charset=utf-8><img alt=x src=\"https://cdn.example.com/p?id=1&amp;size=2\">", url));
        assert!(drag_html_shows_image(b"<IMG SRC='https://cdn.example.com/p?id=1&size=2'>", url));
        assert!(!drag_html_shows_image(b"<a href=\"https://cdn.example.com/p?id=1&size=2\">link</a>", url), "a link isn't a picture");
        assert!(!drag_html_shows_image(b"<img alt=\"src=https://cdn.example.com/p?id=1&size=2\">", url), "inside another value");
        assert!(!drag_html_shows_image(b"<img src=\"https://cdn.example.com/p?id=1&size=23\">", url));
        let big = format!("<img src=\"{url}\">{}", " ".repeat(DRAG_HTML_BYTES));
        assert!(!drag_html_shows_image(big.as_bytes(), url), "too big to be an image drag");
    }

    /// This thread's CPU time (other tests run beside it), or the wall clock
    /// where there's no per-thread clock to read.
    pub(crate) fn cpu_seconds() -> f64 {
        #[cfg(unix)]
        {
            let mut at: libc::timespec = unsafe { std::mem::zeroed() };
            // SAFETY: clock_gettime only writes the struct it's given.
            assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut at) }, 0);
            at.tv_sec as f64 + at.tv_nsec as f64 / 1e9
        }
        #[cfg(not(unix))]
        {
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
        }
    }

    /// Daybreak 120: 40 KB of `<img ` took five seconds in the regex. The
    /// whole allowance of it, and of other unclosed shapes, is quick now.
    #[test]
    fn unclosed_tags_are_read_in_one_pass() {
        let url = "https://example.com/picture";
        for html in ["<img ".repeat(DRAG_HTML_BYTES / 5), "<img src=x ".repeat(DRAG_HTML_BYTES / 11), "<img a=\"".repeat(DRAG_HTML_BYTES / 8), "<".repeat(DRAG_HTML_BYTES)] {
            let started = cpu_seconds();
            assert!(!drag_html_shows_image(html.as_bytes(), url));
            let spent = cpu_seconds() - started;
            assert!(spent < 0.1, "{}... took {spent}s", &html[..10]);
        }
    }
}

#[cfg(test)]
mod drag_source {
    const SOURCE: &str = include_str!("../../gpui/native/composer_media.m");

    /// What the drag path must and mustn't say. Line endings are made `\n`
    /// first: Git for Windows checks the file out with `\r\n` (autocrlf), and
    /// the budget check spans two lines, so Windows CI failed on it alone.
    fn check_drag_source(source: &str) {
        let source = source.replace("\r\n", "\n");
        assert!(!source.contains("NSRegularExpression"), "composer_media.m uses a regex again");
        assert!(source.contains("if (!html) return false;\n  if (html.length > *budget) { *budget = 0; return false; }"), "one HTML budget for a drag's items, which an item with no HTML doesn't spend");
        assert!(source.contains("if (++seen > TB_DRAG_ITEMS) break;"));
        assert!(!source.contains("imageURL(item)"), "every caller passes the budget");
        assert!(source.contains("tb_drag_html_shows_image(markup.bytes"));
    }

    /// The drag's HTML goes only through the one-pass reader: no regex comes
    /// back into the main-thread drag path (Daybreak 120).
    #[test]
    fn the_composer_drag_reads_html_only_through_the_one_pass_reader() {
        check_drag_source(SOURCE);
    }

    /// The same checks hold on a Windows checkout of the file.
    #[test]
    fn the_drag_source_checks_hold_with_windows_line_endings() {
        check_drag_source(&SOURCE.replace("\r\n", "\n").replace('\n', "\r\n"));
    }

    /// The checks still catch a regex coming back, whatever the line endings.
    #[test]
    #[should_panic(expected = "uses a regex again")]
    fn the_drag_source_checks_still_catch_a_regex() {
        check_drag_source(&format!("{}\r\nNSRegularExpression *re;\r\n", SOURCE.replace('\n', "\r\n")));
    }
}
