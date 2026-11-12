// Words in a message that stand for something the app can show as a chip: a
// picture pasted among the words, a Notion card or another thread linked, a
// thread named by its id (Swift's InlinePictures.markers, CardMentions and
// ThreadRefs). The app draws each as a chip; this finds them, and says what a
// thread chip's hover card says.
use std::collections::HashMap;
use std::ops::Range;

// ---------------------------------------------------------------- pictures

/// The `[Picture N]` markers in a message that place one of its
/// `image_count` pictures, in the order they come: the first naming each
/// picture, as the runner reads them (runner/src/pictures.ts). A marker
/// naming no picture, or one already placed, is words. Byte ranges, and
/// each picture's index (N - 1).
pub fn picture_markers(text: &str, image_count: usize) -> Vec<(Range<usize>, usize)> {
    const OPEN: &str = "[Picture ";
    let mut found = Vec::new();
    if image_count == 0 || !text.contains(OPEN) {
        return found;
    }
    let mut placed = vec![false; image_count];
    let mut from = 0;
    while let Some(at) = text[from..].find(OPEN).map(|n| from + n) {
        let digits_at = at + OPEN.len();
        let digits = text[digits_at..].bytes().take_while(u8::is_ascii_digit).count();
        from = digits_at;
        if !(1..=3).contains(&digits) || text.as_bytes().get(digits_at + digits) != Some(&b']') {
            continue;
        }
        let Ok(number) = text[digits_at..digits_at + digits].parse::<usize>() else { continue };
        let end = digits_at + digits + 1;
        from = end;
        if number == 0 || number > image_count || placed[number - 1] {
            continue;
        }
        placed[number - 1] = true;
        found.push((at..end, number - 1));
    }
    found
}

// ---------------------------------------------------------------- cards and threads

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MentionKind {
    /// A Notion card, by its page's address.
    Card,
    /// Another thread, as a `thread:<id>` link, which opens in the app.
    Thread,
}

/// A Notion card or another thread mentioned in a message. A message always
/// holds a plain Markdown link (`[title](https://…notion.so/…)` or
/// `[title](thread:<id>)`), so the agent gets an address it can read and
/// the phone and the archive get ordinary words; the bubble draws a chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    /// The whole link in the text, as bytes.
    pub range: Range<usize>,
    pub title: String,
    pub url: String,
    pub kind: MentionKind,
}

impl Mention {
    /// The thread a `thread:` mention names.
    pub fn thread_id(&self) -> Option<&str> {
        self.url.strip_prefix("thread:")
    }
}

/// The mentions in `text`, in order (CardMentions.find).
///
/// A forward scan, not a regex: with `[` allowed in a title and no `](`
/// after them, every `[` started a match that ran to the end of the line,
/// and 32,000 of them took 16 seconds on the Mac (Daybreak, 7 Nov 2026). It
/// finds a title (escaped brackets allowed in it), then an https address on
/// Notion's own domains or a `thread:` link. Nothing but the link's end or a
/// space ends the address, and the domain has to finish where the path
/// starts, so notion.so.example.com is no card. Each character is looked at
/// a few times at most.
pub fn find_mentions(text: &str) -> Vec<Mention> {
    if !text.contains("notion.") && !text.contains("](thread:") {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let n = chars.len();
    // Where an address from each place would stop: at the next `)` or space.
    // An address can hold `[` and `](`, so the addresses of links that fail
    // overlap, and scanning each to its end again was quadratic too.
    let mut stop = vec![n; n + 1];
    for k in (0..n).rev() {
        stop[k] = if chars[k] == ')' || is_space(chars[k]) { k } else { stop[k + 1] };
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i < n {
        if chars[i] != '[' {
            i += 1;
            continue;
        }
        // Where a title from here ends: at the first `]` not escaped, or
        // nowhere if a line ends first. Every `[` before that end gives the
        // same one, so they're all settled by this one scan.
        let mut k = i + 1;
        let mut end = None;
        while k < n {
            match chars[k] {
                ']' => {
                    end = Some(k);
                    break;
                }
                '\n' => break,
                '\\' => {
                    // An escape takes any one character but a line break.
                    if k + 1 >= n || is_line_break(chars[k + 1]) {
                        break;
                    }
                    k += 2;
                }
                _ => k += 1,
            }
        }
        let Some(end) = end else {
            i = k.max(i + 1);
            continue;
        };
        // The first of those `[`s not after a `!` (a picture) with a title
        // left between it and the end.
        let start = (i..end.saturating_sub(1)).find(|&j| chars[j] == '[' && (j == 0 || chars[j - 1] != '!'));
        match (start, destination(&chars, end + 1, &stop)) {
            (Some(start), Some((url, after))) => {
                let title: String = chars[start + 1..end].iter().collect::<String>().replace("\\[", "[").replace("\\]", "]");
                let url: String = chars[url].iter().collect();
                let kind = if url.starts_with("thread:") { MentionKind::Thread } else { MentionKind::Card };
                found.push(Mention { range: offsets[start]..offsets[after], title, url, kind });
                i = after;
            }
            _ => i = end + 1,
        }
    }
    found
}

/// `(https://…notion.so/…)` or `(thread:<id>)` from `at`: the address's
/// place and where the link ends, just past its `)`.
fn destination(u: &[char], at: usize, stop: &[usize]) -> Option<(Range<usize>, usize)> {
    let n = u.len();
    if at >= n || u[at] != '(' {
        return None;
    }
    let begin = at + 1;
    let has = |word: &str, index: usize| word.chars().enumerate().all(|(o, c)| u.get(index + o) == Some(&c));
    let mut k;
    if has("thread:", begin) {
        k = begin + "thread:".len();
        let id_start = k;
        while k < n && is_id_char(u[k]) {
            k += 1;
        }
        if k == id_start {
            return None;
        }
    } else if has("https://", begin) {
        let host = begin + "https://".len();
        k = host;
        while k < n && is_host_char(u[k]) {
            k += 1;
        }
        let name: String = u[host..k.min(n)].iter().collect();
        if k >= n || u[k] != '/' || !is_notion_host(&name) {
            return None;
        }
        k = stop[k];
    } else {
        return None;
    }
    if k >= n || u[k] != ')' {
        return None;
    }
    Some((begin..k, k + 1))
}

/// `(?:[A-Za-z0-9-]+\.)*notion\.(?:so|site|com)`, exactly.
fn is_notion_host(host: &str) -> bool {
    ["notion.so", "notion.site", "notion.com"].iter().any(|top| match host.strip_suffix(top) {
        Some("") => true,
        // Each label before it has a character at least.
        Some(labels) => labels.strip_suffix('.').is_some_and(|l| l.split('.').all(|part| !part.is_empty())),
        None => false,
    })
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '.'
}

/// What the Swift regex's `\s` matched (ICU's White_Space), so an address
/// ends where it did there.
fn is_space(c: char) -> bool {
    matches!(c, '\u{09}'..='\u{0D}' | ' ' | '\u{85}' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// What `.` doesn't match after a backslash.
fn is_line_break(c: char) -> bool {
    matches!(c, '\u{0A}' | '\u{0B}' | '\u{0C}' | '\u{0D}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

// ---------------------------------------------------------------- thread ids

/// Where each id-shaped word is in `text` (ThreadRefs.candidates): lowercase
/// hex, 8 characters or a whole UUID, not inside a longer word or path.
/// Agents name threads by the 8-character prefix `tb status` and `tb list`
/// print, or by the whole id. Only a shape: 8 hex digits could as well be a
/// commit, so `ThreadIndex::resolve` decides what's a thread. Byte ranges.
pub fn thread_ref_candidates(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    if bytes.len() < 8 {
        return found;
    }
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let before_ok = |i: usize| i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'_' | b'-' | b'/' | b'.'));
    let after_ok = |i: usize| i >= bytes.len() || !(bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-' | b'/'));
    let run = |from: usize, len: usize| from + len <= bytes.len() && bytes[from..from + len].iter().all(|&b| hex(b));
    let mut i = 0;
    while i + 8 <= bytes.len() {
        if !hex(bytes[i]) || !before_ok(i) {
            i += 1;
            continue;
        }
        let whole = run(i, 8)
            && [(8, 4), (13, 4), (18, 4), (23, 12)].iter().all(|&(at, len)| bytes.get(i + at) == Some(&b'-') && run(i + at + 1, len))
            && after_ok(i + 36);
        if whole {
            found.push(i..i + 36);
            i += 36;
        } else if run(i, 8) && after_ok(i + 8) {
            found.push(i..i + 8);
            i += 8;
        } else {
            i += 1;
        }
    }
    found
}

/// Thread ids by their full id and by their 8-character prefix, built once
/// per thread list so drawing a chip is a lookup (ThreadRefs.Index).
#[derive(Clone, Debug, Default)]
pub struct ThreadIndex {
    ids: HashMap<String, String>,
}

impl ThreadIndex {
    pub fn new<'a>(thread_ids: impl IntoIterator<Item = &'a str>) -> ThreadIndex {
        let mut ids = HashMap::new();
        // Two threads sharing a prefix: the prefix names neither.
        let mut prefixes: HashMap<String, Option<String>> = HashMap::new();
        // Only ids of a thread id's shape: a chip's link is written with the
        // whole id in it, and another runner's ids are its own word.
        for id in thread_ids.into_iter().filter(|id| crate::thread_link::is_thread_id(id)) {
            ids.insert(id.to_owned(), id.to_owned());
            let prefix: String = id.chars().take(8).collect();
            prefixes.entry(prefix).and_modify(|seen| *seen = None).or_insert_with(|| Some(id.to_owned()));
        }
        for (prefix, id) in prefixes {
            if let Some(id) = id {
                ids.entry(prefix).or_insert(id);
            }
        }
        ThreadIndex { ids }
    }

    /// The thread `token` names, if any.
    pub fn resolve(&self, token: &str) -> Option<&str> {
        self.ids.get(token).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// The hover card's second line (ThreadRefs.detail): where the thread is and
/// what it's doing, "misc · claude-2 · Working 4m".
pub fn thread_detail(folder: &str, account: Option<&str>, status: &str) -> String {
    let mut parts = vec![folder.to_owned()];
    if let Some(account) = account.filter(|a| !a.is_empty()) {
        parts.push(account.to_owned());
    }
    parts.push(status.to_owned());
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

// ---------------------------------------------------------------- a message's pieces

/// A message's words cut where its chips go, in order: Mike's bubble draws
/// the words as words and each chip as a chip (RowRenderer.userText).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Words(String),
    /// The message's picture at this index (`[Picture index+1]`).
    Picture(usize),
    Mention(Mention),
}

/// `text`'s pieces with its pictures placed (of `image_count`) and its card
/// and thread links as mentions. Pictures are found first, as the Swift
/// bubble swaps them out before it looks for links.
pub fn message_pieces(text: &str, image_count: usize) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let words = |piece: &str, pieces: &mut Vec<Piece>| {
        if piece.is_empty() {
            return;
        }
        let mut at = 0;
        for mention in find_mentions(piece) {
            if mention.range.start > at {
                pieces.push(Piece::Words(piece[at..mention.range.start].to_owned()));
            }
            at = mention.range.end;
            pieces.push(Piece::Mention(mention));
        }
        if at < piece.len() {
            pieces.push(Piece::Words(piece[at..].to_owned()));
        }
    };
    let mut at = 0;
    for (range, index) in picture_markers(text, image_count) {
        words(&text[at..range.start], &mut pieces);
        pieces.push(Piece::Picture(index));
        at = range.end;
    }
    words(&text[at..], &mut pieces);
    pieces
}

/// Whether a message has anything to draw as a chip.
pub fn has_chips(text: &str, image_count: usize) -> bool {
    message_pieces(text, image_count).iter().any(|p| !matches!(p, Piece::Words(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<&str> {
        thread_ref_candidates(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn finds_prefixes_and_whole_ids() {
        assert_eq!(words("`85735a8f` (subagents) and 1d827b8c."), ["85735a8f", "1d827b8c"]);
        assert_eq!(words("thread 52594bb9-e49d-4ec7-8ed8-bbc61b48ef77 is this one"), ["52594bb9-e49d-4ec7-8ed8-bbc61b48ef77"]);
        assert_eq!(words("(85735a8f)"), ["85735a8f"]);
        assert_eq!(words("é85735a8f"), ["85735a8f"]);
    }

    /// Commits are 7 characters, longer hashes and paths aren't ids, and
    /// uppercase hex isn't how the runner writes them.
    #[test]
    fn skips_what_isnt_an_id() {
        assert!(words("merged d1ecf9c as baa45b6").is_empty());
        assert!(words("sha 1e66c6d4f2a8b9c0").is_empty());
        assert!(words("releases/20261027T055446Z-d1ecf9c").is_empty());
        assert!(words("app/.fixtures/threads/85735a8f").is_empty());
        assert!(words("x85735a8f 85735a8fx 85735A8F").is_empty());
        assert!(words("52594bb9-e49d-4ec7-8ed8-bbc61b48ef77x").is_empty());
    }

    #[test]
    fn the_index_resolves_unique_prefixes() {
        let index = ThreadIndex::new([
            "52594bb9-e49d-4ec7-8ed8-bbc61b48ef77",
            "85735a8f-0000-4000-8000-000000000001",
            "aaaaaaaa-0000-4000-8000-000000000001",
            "aaaaaaaa-0000-4000-8000-000000000002",
        ]);
        assert_eq!(index.resolve("52594bb9"), Some("52594bb9-e49d-4ec7-8ed8-bbc61b48ef77"));
        assert_eq!(index.resolve("52594bb9-e49d-4ec7-8ed8-bbc61b48ef77"), Some("52594bb9-e49d-4ec7-8ed8-bbc61b48ef77"));
        assert_eq!(index.resolve("85735a8f"), Some("85735a8f-0000-4000-8000-000000000001"));
        assert_eq!(index.resolve("aaaaaaaa"), None, "two threads share it");
        assert_eq!(index.resolve("aaaaaaaa-0000-4000-8000-000000000002"), Some("aaaaaaaa-0000-4000-8000-000000000002"));
        assert_eq!(index.resolve("1d827b8c"), None);
    }

    #[test]
    fn the_detail_line() {
        assert_eq!(thread_detail("threadbear", Some("claude-2"), "Working 4m"), "threadbear · claude-2 · Working 4m");
        assert_eq!(thread_detail("threadbear", None, "Needs you"), "threadbear · Needs you");
        assert_eq!(thread_detail("", Some(""), "Archived"), "Archived");
    }

    #[test]
    fn markers_place_the_first_naming_each_picture() {
        let text = "[Picture 2] then [Picture 1], [Picture 2] again, [Picture 9] and [picture 1] [Picture 1234]";
        let found = picture_markers(text, 2);
        assert_eq!(found.iter().map(|(_, i)| *i).collect::<Vec<_>>(), [1, 0]);
        assert_eq!(found.iter().map(|(r, _)| &text[r.clone()]).collect::<Vec<_>>(), ["[Picture 2]", "[Picture 1]"]);
        assert!(picture_markers(text, 0).is_empty());
        assert!(picture_markers("[Picture 0]", 1).is_empty());
    }

    #[test]
    fn card_mentions_are_notion_links_in_a_message() {
        let text = "Look at [Fix \\[the\\] button](https://app.notion.com/p/abc123) and [Docs](https://example.com/x) please";
        let found = find_mentions(text);
        assert_eq!(found.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), ["Fix [the] button"], "only Notion links, with the brackets back");
        assert_eq!(found[0].url, "https://app.notion.com/p/abc123");
        assert_eq!(&text[found[0].range.clone()], "[Fix \\[the\\] button](https://app.notion.com/p/abc123)");
        let titles = |t: &str| find_mentions(t).into_iter().map(|m| m.title).collect::<Vec<_>>();
        assert_eq!(titles("[a](https://www.notion.so/t3) [b](https://x.notion.site/y)"), ["a", "b"]);
        assert!(find_mentions("![a picture](https://www.notion.so/t3)").is_empty(), "a picture isn't a card");
        assert!(find_mentions("[a](https://notnotion.so/t3) [b](http://www.notion.so/t3) [c](https://notion.so.evil.com/t3)").is_empty(), "only real Notion addresses");
        assert!(find_mentions("no links here").is_empty());
        assert!(find_mentions("[a](https://.notion.so/x) [b](https://a..notion.so/x)").is_empty());
    }

    #[test]
    fn thread_links_are_mentions_too() {
        let found = find_mentions("See [Fix \\[the\\] chip](thread:0cf430ac-f429) and [card](https://notion.so/a)");
        assert_eq!(found.iter().map(|m| m.kind).collect::<Vec<_>>(), [MentionKind::Thread, MentionKind::Card]);
        assert_eq!(found[0].title, "Fix [the] chip");
        assert_eq!(found[0].url, "thread:0cf430ac-f429");
        assert_eq!(found[0].thread_id(), Some("0cf430ac-f429"));
        assert!(find_mentions("[x](thread:a b) [y](threads:a) ![z](thread:a)").is_empty());
    }

    #[test]
    fn many_brackets_stay_quick() {
        let started = std::time::Instant::now();
        find_mentions(&format!("{}](thread:", "[".repeat(32_000)));
        find_mentions(&format!("[a]({}notion.so/", "https://".repeat(4_000)));
        assert!(started.elapsed().as_secs_f64() < 2., "took {:?}", started.elapsed());
    }

    #[test]
    fn a_message_cuts_into_words_and_chips() {
        let pieces = message_pieces("Here [Picture 1] and [Card](https://www.notion.so/a) for [Morning brief](thread:abc) [Picture 3]", 2);
        assert_eq!(pieces.len(), 7);
        assert_eq!(pieces[0], Piece::Words("Here ".into()));
        assert_eq!(pieces[1], Piece::Picture(0));
        assert!(matches!(&pieces[3], Piece::Mention(m) if m.kind == MentionKind::Card && m.title == "Card"));
        assert!(matches!(&pieces[5], Piece::Mention(m) if m.kind == MentionKind::Thread && m.thread_id() == Some("abc")));
        assert_eq!(pieces[6], Piece::Words(" [Picture 3]".into()), "a marker naming no picture is words");
        assert!(has_chips("see [Picture 1]", 1));
        assert!(!has_chips("see [Picture 1]", 0));
        assert!(!has_chips("plain words", 0));
    }
}
