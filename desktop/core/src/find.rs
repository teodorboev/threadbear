// The find bar over a file in the side pane, with no UI in it: the search
// AppKit's find bar does over the Swift app's text view (NSTextFinder's
// defaults, Contains and Ignore Case, wrapping round at either end), and what
// the bar says about it. The code editor has the same search built in
// (gpui-kit's, which this matches for the words people type); the drawn
// Markdown page uses this one (desktop/gpui/src/file_find.rs).
use std::ops::Range;

/// Where `query` is in `text`, as byte ranges, left to right and not
/// overlapping, ignoring case. Nothing for an empty query.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    if query.is_ascii() {
        // An ASCII byte is never part of a longer UTF-8 character, so a match
        // here always starts and ends on a character.
        let (hay, needle) = (text.as_bytes(), query.as_bytes());
        let mut at = 0;
        while at + needle.len() <= hay.len() {
            if hay[at..at + needle.len()].eq_ignore_ascii_case(needle) {
                out.push(at..at + needle.len());
                at += needle.len();
            } else {
                at += 1;
            }
        }
        return out;
    }
    let folded: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut after = 0;
    for (start, _) in text.char_indices() {
        if start < after {
            continue;
        }
        if let Some(end) = match_at(text, start, &folded) {
            out.push(start..end);
            after = end;
        }
    }
    out
}

/// The end of a match of `folded` (a query in lower case) at `start`, if there's one.
fn match_at(text: &str, start: usize, folded: &[char]) -> Option<usize> {
    let mut wanted = folded.iter();
    for (offset, c) in text[start..].char_indices() {
        for lower in c.to_lowercase() {
            if wanted.next() != Some(&lower) {
                return None;
            }
        }
        if wanted.len() == 0 {
            return Some(start + offset + c.len_utf8());
        }
    }
    None
}

/// What the bar says beside its words: "3 of 8", "Not found", or nothing
/// before anything's typed. `current` is the match picked, from 0.
pub fn count_label(query: &str, current: Option<usize>, total: usize) -> String {
    if query.is_empty() {
        return String::new();
    }
    match current {
        _ if total == 0 => "Not found".to_owned(),
        Some(index) => format!("{} of {total}", index + 1),
        None => format!("{total} found"),
    }
}

/// The dump's line for the bar, the same on both apps: "the: 3 of 8",
/// "the: not found".
pub fn dump_line(query: &str, current: Option<usize>, total: usize) -> String {
    if query.is_empty() {
        return String::new();
    }
    match current {
        _ if total == 0 => format!("{query}: not found"),
        Some(index) => format!("{query}: {} of {total}", index + 1),
        None => format!("{query}: {total} found"),
    }
}

/// One search through a text: its matches and the one picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finder {
    pub query: String,
    pub matches: Vec<Range<usize>>,
    pub current: Option<usize>,
}

impl Finder {
    /// Searches `text` for `query`, picking the first match that starts at or
    /// after `from` (round to the first if none does), as the bar finds the
    /// next one after the selection.
    pub fn search(&mut self, text: &str, query: &str, from: usize) {
        self.query = query.to_owned();
        self.matches = find_all(text, query);
        self.current = if self.matches.is_empty() { None } else { Some(self.matches.iter().position(|m| m.start >= from).unwrap_or(0)) };
    }

    /// The text changed under the search (a reload): the same words again,
    /// keeping to the match picked as near as there's one.
    pub fn refresh(&mut self, text: &str) {
        let from = self.current_range().map_or(0, |r| r.start);
        let query = std::mem::take(&mut self.query);
        self.search(text, &query, from);
    }

    pub fn current_range(&self) -> Option<Range<usize>> {
        self.current.and_then(|i| self.matches.get(i).cloned())
    }

    /// ⌘G, Return: the next match, round to the first after the last.
    pub fn next(&mut self) -> Option<Range<usize>> {
        let count = self.matches.len();
        if count == 0 {
            return None;
        }
        self.current = Some(self.current.map_or(0, |i| (i + 1) % count));
        self.current_range()
    }

    /// ⇧⌘G, Shift-Return: the one before, round to the last before the first.
    pub fn previous(&mut self) -> Option<Range<usize>> {
        let count = self.matches.len();
        if count == 0 {
            return None;
        }
        self.current = Some(self.current.map_or(count - 1, |i| (i + count - 1) % count));
        self.current_range()
    }

    pub fn label(&self) -> String {
        count_label(&self.query, self.current, self.matches.len())
    }

    pub fn dump_line(&self) -> String {
        dump_line(&self.query, self.current, self.matches.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVIEW: &str = "The new hero is shorter, and every page now leads with what the app does.\nThe three cards say what you get.";

    #[test]
    fn finds_words_whatever_their_case_and_not_overlapping() {
        assert_eq!(find_all(REVIEW, "the"), [0..3, 60..63, 74..77]);
        assert_eq!(find_all("aaaa", "aa"), [0..2, 2..4]);
        assert!(find_all(REVIEW, "").is_empty());
        assert!(find_all(REVIEW, "zzz").is_empty());
        // A word ending the text, and one longer than it.
        assert_eq!(find_all("ends with words", "WORDS"), [10..15]);
        assert!(find_all("ab", "abc").is_empty());
    }

    #[test]
    fn finds_words_with_letters_past_ascii() {
        let text = "Café, CAFÉ and café au lait";
        assert_eq!(find_all(text, "café"), [0..5, 7..12, 17..22]);
        // Byte ranges, so a match after a wide character lands on its characters.
        assert_eq!(&text[find_all(text, "lait")[0].clone()], "lait");
        assert_eq!(find_all("日本語のテキスト", "テキ"), [12..18]);
    }

    #[test]
    fn steps_round_at_both_ends() {
        let mut finder = Finder::default();
        finder.search(REVIEW, "the", 0);
        assert_eq!(finder.label(), "1 of 3");
        assert_eq!(finder.next(), Some(60..63));
        assert_eq!(finder.next(), Some(74..77));
        assert_eq!(finder.label(), "3 of 3");
        assert_eq!(finder.next(), Some(0..3), "round to the first");
        assert_eq!(finder.previous(), Some(74..77), "and back round to the last");
        assert_eq!(finder.dump_line(), "the: 3 of 3");
    }

    #[test]
    fn a_search_starts_after_where_the_last_one_was() {
        let mut finder = Finder::default();
        finder.search(REVIEW, "the", 61);
        assert_eq!(finder.current_range(), Some(74..77));
        finder.search(REVIEW, "the", 100);
        assert_eq!(finder.current, Some(0), "past the last match goes round to the first");
        // A reload keeps to the match picked.
        finder.search(REVIEW, "the", 61);
        finder.refresh(&format!("Intro.\n{REVIEW}"));
        assert_eq!(finder.current_range(), Some(81..84));
    }

    #[test]
    fn the_bar_says_how_many() {
        assert_eq!(count_label("", None, 0), "");
        assert_eq!(count_label("zzz", None, 0), "Not found");
        assert_eq!(count_label("the", Some(1), 8), "2 of 8");
        assert_eq!(dump_line("zzz", None, 0), "zzz: not found");
        let mut finder = Finder::default();
        finder.search(REVIEW, "zzz", 0);
        assert_eq!(finder.next(), None);
        assert_eq!(finder.previous(), None);
        assert_eq!(finder.label(), "Not found");
    }
}
