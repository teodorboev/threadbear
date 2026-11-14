//! The message box's spelling and substitutions, as the Swift app's
//! NSTextView gives them: what to mark, what to correct after a word's typed,
//! and the Edit menu's Transformations. The Mac's spell checker
//! (`gpui/src/text_services.rs`) finds the words; this decides what's done
//! with what it found, so it's tested without a Mac.
use std::ops::Range;

/// What the spell checker found, by kind (NSTextCheckingType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spelling,
    Grammar,
    /// An autocorrection of a misspelled word.
    Correction,
    /// The Mac's text replacements (Keyboard > Text Replacements).
    Replacement,
    Quote,
    Dash,
}

/// One thing found, with a byte range into the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub kind: Kind,
    /// What to put in its place (corrections, replacements, quotes, dashes).
    pub replacement: Option<String>,
}

/// Which marks to draw, `true` for grammar (green) and `false` for spelling
/// (red). As NSTextView, the word the caret is in or just after isn't marked
/// while it's being typed; and nothing inside `skip` (a picture's chip) is.
pub fn marks(found: &[Found], caret: Option<usize>, skip: &[Range<usize>]) -> Vec<(Range<usize>, bool)> {
    found
        .iter()
        .filter_map(|f| match f.kind {
            Kind::Spelling => Some((f.range.clone(), false)),
            Kind::Grammar => Some((f.range.clone(), true)),
            _ => None,
        })
        .filter(|(r, grammar)| *grammar || !caret.is_some_and(|c| r.start <= c && c <= r.end))
        .filter(|(r, _)| !skip.iter().any(|s| s.start < r.end && r.start < s.end))
        .collect()
}

/// The one character typed to make `new` from `old`, ending at `caret`, when
/// it ends a word (a space, a new line or punctuation): what sets off an
/// autocorrection or a text replacement, and a quote or dash substitution.
pub fn typed_boundary(old: &str, new: &str, caret: usize) -> Option<char> {
    if new.len() <= old.len() || caret > new.len() || !new.is_char_boundary(caret) {
        return None;
    }
    let typed = new[..caret].chars().next_back()?;
    let start = caret - typed.len_utf8();
    if new.len() - old.len() != typed.len_utf8() || new[..start] != old[..start] || new[caret..] != old[start..] {
        return None;
    }
    ends_word(typed).then_some(typed)
}

fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '"' | '\'' | '-')
}

/// The substitutions to make now that `typed` was typed just before `caret`:
/// a correction or replacement of the word right before it, a dash made of
/// what's right before it, or the quote just typed. Each as a byte range and
/// what goes there, last first, so applying them in order keeps the earlier
/// ranges right.
pub fn substitutions(found: &[Found], caret: usize, typed: char) -> Vec<(Range<usize>, String)> {
    let before = caret - typed.len_utf8();
    let mut out: Vec<(Range<usize>, String)> = found
        .iter()
        .filter_map(|f| {
            let with = f.replacement.clone()?;
            let fits = match f.kind {
                Kind::Correction | Kind::Replacement => f.range.end == before && !matches!(typed, '"' | '\'' | '-'),
                Kind::Dash => f.range.end == before || f.range.end == caret,
                Kind::Quote => f.range == (before..caret),
                Kind::Spelling | Kind::Grammar => false,
            };
            (fits && f.range.start < f.range.end).then(|| (f.range.clone(), with))
        })
        .collect();
    out.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    // A replacement and a correction of the same word: the replacement, as
    // the Mac does it.
    out.dedup_by(|later, earlier| later.0 == earlier.0);
    out
}

/// The marks of `old` moved to where their words are in `new`, as the box
/// is edited between checks: one changed stretch is found (what's the same
/// at the start and the end), marks after it shift by what it grew or shrank,
/// and a mark it touches goes until the next check.
pub fn shift_marks(marks: &[(Range<usize>, bool)], old: &str, new: &str) -> Vec<(Range<usize>, bool)> {
    let prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '\'' || c == '’');
    marks
        .iter()
        .filter_map(|(r, grammar)| {
            if r.end <= prefix {
                Some((r.clone(), *grammar))
            } else if r.start >= old_end {
                Some(((r.start - old_end + new_end)..(r.end - old_end + new_end), *grammar))
            } else {
                None
            }
        })
        .filter(|(r, _)| new.is_char_boundary(r.start) && new.is_char_boundary(r.end))
        // A word typed onto at either end is a different word now.
        .filter(|(r, _)| !wordy(new[r.end..].chars().next()) && !wordy(new[..r.start].chars().next_back()))
        .collect()
}

/// Byte offset in `text` of a UTF-16 offset (an NSRange's), clipped to the end.
pub fn byte_of_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// UTF-16 offset of a byte offset in `text`.
pub fn utf16_of_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().map(char::len_utf16).sum()
}

/// Edit > Transformations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    Upper,
    Lower,
    Capitalize,
}

impl Transform {
    pub fn apply(self, text: &str) -> String {
        match self {
            Transform::Upper => text.to_uppercase(),
            Transform::Lower => text.to_lowercase(),
            // NSString's capitalizedString: each word's first letter up, the
            // rest down.
            Transform::Capitalize => {
                let mut out = String::with_capacity(text.len());
                let mut start = true;
                for c in text.chars() {
                    if c.is_alphanumeric() {
                        if start {
                            out.extend(c.to_uppercase());
                        } else {
                            out.extend(c.to_lowercase());
                        }
                        start = false;
                    } else {
                        out.push(c);
                        start = c != '\'' && c != '’';
                    }
                }
                out
            }
        }
    }
}

/// What a transformation or Start Speaking works on: the selection, or with
/// none, the word the caret is in (NSTextView's own rule for
/// Transformations).
pub fn word_at(text: &str, at: usize) -> Range<usize> {
    let at = at.min(text.len());
    let in_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '_';
    let start = text[..at].char_indices().rev().take_while(|(_, c)| in_word(*c)).last().map_or(at, |(i, _)| i);
    let end = text[at..].char_indices().find(|(_, c)| !in_word(*c)).map_or(text.len(), |(i, _)| at + i);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(range: Range<usize>, kind: Kind, with: Option<&str>) -> Found {
        Found { range, kind, replacement: with.map(str::to_owned) }
    }

    #[test]
    fn the_word_being_typed_isnt_marked() {
        let f = [found(0..3, Kind::Spelling, None), found(4..8, Kind::Spelling, None)];
        // "teh wrod" with the caret at the end: only "teh".
        assert_eq!(marks(&f, Some(8), &[]), vec![(0..3, false)]);
        // Once the caret leaves, both.
        assert_eq!(marks(&f, Some(9), &[]), vec![(0..3, false), (4..8, false)]);
        assert_eq!(marks(&f, None, &[]).len(), 2);
    }

    #[test]
    fn chips_and_substitutions_arent_marked() {
        let f = [found(0..3, Kind::Spelling, None), found(5..9, Kind::Grammar, None), found(10..12, Kind::Correction, Some("x"))];
        assert_eq!(marks(&f, None, &[1..2]), vec![(5..9, true)]);
    }

    #[test]
    fn a_typed_space_is_found() {
        assert_eq!(typed_boundary("hi teh", "hi teh ", 7), Some(' '));
        assert_eq!(typed_boundary("hi teh", "hi teh.", 7), Some('.'));
        // A letter doesn't end a word, and a paste isn't typing.
        assert_eq!(typed_boundary("hi te", "hi teh", 6), None);
        assert_eq!(typed_boundary("hi", "hi there ", 9), None);
        // Typed in the middle.
        assert_eq!(typed_boundary("teh end", "teh  end", 4), Some(' '));
        assert_eq!(typed_boundary("abc", "abc", 3), None);
        assert_eq!(typed_boundary("é", "é ", 3), Some(' '));
    }

    #[test]
    fn only_the_word_before_the_space_is_corrected() {
        let f = [
            found(0..3, Kind::Correction, Some("the")),
            found(4..8, Kind::Correction, Some("word")),
            found(9..12, Kind::Spelling, None),
        ];
        // "teh wrod " typed: the space at 8..9.
        assert_eq!(substitutions(&f, 9, ' '), vec![(4..8, "word".to_owned())]);
    }

    #[test]
    fn a_replacement_wins_over_a_correction() {
        let f = [found(0..3, Kind::Replacement, Some("on my way")), found(0..3, Kind::Correction, Some("omg"))];
        assert_eq!(substitutions(&f, 4, ' ').len(), 1);
    }

    #[test]
    fn quotes_and_dashes() {
        let f = [found(3..4, Kind::Quote, Some("“")), found(0..2, Kind::Dash, Some("—"))];
        assert_eq!(substitutions(&f, 4, '"'), vec![(3..4, "“".to_owned())]);
        assert_eq!(substitutions(&f, 3, ' '), vec![(0..2, "—".to_owned())]);
    }

    #[test]
    fn marks_move_with_an_edit() {
        let marks = [(0..3, false), (8..12, false)];
        // "teh and wrod" with "big " typed at the start.
        assert_eq!(shift_marks(&marks, "teh and wrod", "big teh and wrod"), vec![(4..7, false), (12..16, false)]);
        // Typing inside a marked word drops its mark until the next check.
        assert_eq!(shift_marks(&marks, "teh and wrod", "tehh and wrod"), vec![(9..13, false)]);
        // Deleting before one moves it back.
        assert_eq!(shift_marks(&marks, "teh and wrod", "teh  wrod"), vec![(0..3, false), (5..9, false)]);
        assert_eq!(shift_marks(&marks, "teh and wrod", "teh and wrod"), marks.to_vec());
    }

    #[test]
    fn utf16_offsets() {
        let t = "a😀b";
        assert_eq!(byte_of_utf16(t, 1), 1);
        assert_eq!(byte_of_utf16(t, 3), 5);
        assert_eq!(byte_of_utf16(t, 99), 6);
        assert_eq!(utf16_of_byte(t, 5), 3);
    }

    #[test]
    fn transformations() {
        assert_eq!(Transform::Capitalize.apply("hello wORLD it's"), "Hello World It's");
        assert_eq!(Transform::Upper.apply("straße"), "STRASSE");
        assert_eq!(Transform::Lower.apply("ABC"), "abc");
    }

    #[test]
    fn the_word_at_the_caret() {
        assert_eq!(word_at("hello world", 7), 6..11);
        assert_eq!(word_at("hello world", 5), 0..5);
        assert_eq!(word_at("it's here", 1), 0..4);
        assert_eq!(word_at("a  b", 2), 2..2);
    }
}
