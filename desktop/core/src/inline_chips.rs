// The composer's chips other than a picture's, and what a copy of chips takes
// with it: a Notion card or another thread among the words (CardChips.swift's
// CardAttachment), a thread's id pasted on its own (CardMentions.pastedThreadId
// and threadReference), and the words and pictures a copy or cut of the box
// carries (ComposerTextView's Clip). The GPUI composer draws what's here
// (desktop/gpui/src/composer_inline.rs).
use crate::chips::{find_mentions, Mention};
use crate::composer::{draft_words, message_text, PICTURE_MARK};
use crate::reducer::ImageRef;
use serde_json::{json, Value};
use std::ops::Range;

/// How a pasted thread id goes into a message: a link the agent can read
/// the id from, and the app draws as a chip with the thread's title
/// (CardMentions.threadReference). The title's brackets are escaped, so the
/// link still reads as one.
pub fn thread_reference(id: &str, title: &str) -> String {
    // Swift's `split(whereSeparator: \.isNewline)`, which leaves out empty lines.
    let newline = |c: char| matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}');
    let words = title.split(newline).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    let words = words.trim_matches(|c: char| c == ' ' || c == '\t');
    let name = if words.is_empty() { "Thread" } else { words };
    format!("[{}](thread:{id})", name.replace('[', "\\[").replace(']', "\\]"))
}

/// The thread a paste names on its own (CardMentions.pastedThreadId): a bare
/// id (from "Copy thread id"), `thread:<id>` or `threadbear://thread/<id>`.
/// Only a known thread's counts, so any other id pasted stays words.
pub fn pasted_thread_id(pasted: &str, known: impl Fn(&str) -> bool) -> Option<String> {
    let mut text = pasted.trim();
    for prefix in ["threadbear://thread/", "thread:"] {
        if text.len() >= prefix.len() && text.is_char_boundary(prefix.len()) && text[..prefix.len()].eq_ignore_ascii_case(prefix) {
            text = &text[prefix.len()..];
        }
    }
    if text.is_empty() || text.chars().count() > 64 || !text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    if known(text) {
        return Some(text.to_owned());
    }
    let lower = text.to_ascii_lowercase();
    known(&lower).then_some(lower)
}

/// A piece of a draft as the composer shows it: words, the draft's picture
/// at an index, or a card or thread drawn as its chip, with the link it
/// stands for as it was written (CardAttachment.reference).
#[derive(Clone, Debug, PartialEq)]
pub enum DraftPiece {
    Words(String),
    Picture(usize),
    Mention(Mention, String),
}

/// A draft's words cut where its chips go (ComposerTextView.chips(for:)):
/// each mark the next of its `image_count` pictures, a mark with no picture
/// left over dropped, and any card or thread linked in the words between
/// them a chip of its own. A paste of words is a draft with no pictures,
/// whose stray marks go.
pub fn draft_pieces(text: &str, image_count: usize) -> Vec<DraftPiece> {
    let mut pieces = Vec::new();
    let mut run = String::new();
    let mut pictures = 0;
    let flush = |run: &mut String, pieces: &mut Vec<DraftPiece>| {
        if run.is_empty() {
            return;
        }
        let mut last = 0;
        for mention in find_mentions(run) {
            if mention.range.start > last {
                pieces.push(DraftPiece::Words(run[last..mention.range.start].to_owned()));
            }
            last = mention.range.end;
            let link = run[mention.range.clone()].to_owned();
            pieces.push(DraftPiece::Mention(mention, link));
        }
        if last < run.len() {
            pieces.push(DraftPiece::Words(run[last..].to_owned()));
        }
        run.clear();
    };
    for c in text.chars() {
        if c != PICTURE_MARK {
            run.push(c);
        } else if pictures < image_count {
            flush(&mut run, &mut pieces);
            pieces.push(DraftPiece::Picture(pictures));
            pictures += 1;
        }
    }
    flush(&mut run, &mut pieces);
    pieces
}

/// Whether a paste of `text` has anything to draw as a chip: a card or
/// thread linked in it.
pub fn has_mentions(text: &str) -> bool {
    !find_mentions(text).is_empty()
}

/// What a chip in the box stands for, for `draft_in`.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxChip {
    /// A picture: its `ImageRef` once the runner has it, None while it's
    /// still being copied in (it isn't part of the draft yet).
    Picture(Option<ImageRef>),
    /// A card or a thread, whose text in the box is the link it stands for.
    Mention,
}

/// Whether the box holds anything a restart would lose: words, a picture
/// that's in, or one still being copied in, which `draft_in` leaves out as
/// it isn't part of the draft yet (Codex's review, 12 Nov).
pub fn holds_anything(text: &str, chips: &[(Range<usize>, BoxChip)]) -> bool {
    let (words, images) = draft_in(text, chips, 0..text.len());
    !words.trim().is_empty() || !images.is_empty() || chips.iter().any(|(_, c)| matches!(c, BoxChip::Picture(None)))
}

/// The draft in `range` of the box's `text` (ComposerTextView.draft(in:)):
/// the words, a mark for each picture that's in, a card's or thread's link
/// as it is, and those pictures in order. `chips` are the box's chips by
/// their byte ranges, in order; one the range only touches part of counts
/// whole, as a chip can't be cut in two. A stray U+FFFC among the words
/// isn't a picture.
pub fn draft_in(text: &str, chips: &[(Range<usize>, BoxChip)], range: Range<usize>) -> (String, Vec<ImageRef>) {
    let end = range.end.min(text.len());
    let mut at = range.start.min(end);
    let mut out = String::new();
    let mut images = Vec::new();
    if at == end {
        return (out, images);
    }
    for (chip_range, chip) in chips {
        // Only the chips the range reaches into.
        if chip_range.start >= end || chip_range.end <= at {
            continue;
        }
        if chip_range.start > at {
            out.push_str(&draft_words(&text[at..chip_range.start]));
        }
        match chip {
            BoxChip::Picture(Some(image)) => {
                out.push(PICTURE_MARK);
                images.push(image.clone());
            }
            BoxChip::Picture(None) => {}
            BoxChip::Mention => out.push_str(&text[chip_range.clone()]),
        }
        at = chip_range.end.max(at);
    }
    if at < end {
        out.push_str(&draft_words(&text[at..end]));
    }
    (out, images)
}

/// What a copy or cut of chips puts on the clipboard: the words as the agent
/// would get them (`[Picture 1]` for the first picture in it), with the
/// draft and its pictures beside them, so a paste in a composer brings the
/// pictures back, not just their markers (ComposerTextView.Clip).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub plain: String,
    pub text: String,
    pub images: Vec<ImageRef>,
}

impl Clip {
    pub fn new(text: String, images: Vec<ImageRef>) -> Clip {
        Clip { plain: message_text(&text, 1), text, images }
    }

    /// What goes beside the words on the clipboard. Marked as this app's,
    /// so another app's note beside its own words is never read as a clip.
    pub fn metadata(&self) -> Value {
        json!({ "threadbear": "composer-pictures", "text": self.text, "images": self.images.iter().map(image_json).collect::<Vec<_>>() })
    }

    /// A clip back from what was beside `plain` on the clipboard, if it's
    /// one of this app's and still goes with those words.
    pub fn from_clipboard(plain: &str, metadata: &Value) -> Option<Clip> {
        if metadata["threadbear"] != "composer-pictures" {
            return None;
        }
        let text = metadata["text"].as_str()?.to_owned();
        let images: Vec<ImageRef> = metadata["images"].as_array()?.iter().filter_map(ImageRef::decode).collect();
        let clip = Clip::new(text, images);
        // A clipboard keeps its words as they came, but Windows writes line
        // ends as \r\n.
        (clip.plain.replace("\r\n", "\n") == plain.replace("\r\n", "\n")).then_some(clip)
    }
}

/// The JSON a picture goes as (`ImageRef` in docs/protocol.md).
pub fn image_json(image: &ImageRef) -> Value {
    let mut out = json!({ "path": image.path, "mediaType": image.media_type });
    if let Some(w) = image.width {
        out["width"] = json!(w);
    }
    if let Some(h) = image.height {
        out["height"] = json!(h);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chips::MentionKind;

    #[test]
    fn a_thread_reference_is_a_link_with_its_title() {
        assert_eq!(thread_reference("abc-1", "Fix the [share] button"), "[Fix the \\[share\\] button](thread:abc-1)");
        assert_eq!(thread_reference("abc", "two\nlines "), "[two lines](thread:abc)");
        assert_eq!(thread_reference("abc", "  "), "[Thread](thread:abc)");
        // It reads back as the thread's chip, with the title it had.
        let found = find_mentions(&thread_reference("abc", "Fix the [share] button"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Fix the [share] button");
        assert_eq!(found[0].thread_id(), Some("abc"));
    }

    #[test]
    fn only_a_known_threads_id_pasted_on_its_own_counts() {
        let known = |id: &str| id == "0cf430ac-f429-4b1c" || id == "demo_upload";
        assert_eq!(pasted_thread_id("0cf430ac-f429-4b1c", known).as_deref(), Some("0cf430ac-f429-4b1c"));
        assert_eq!(pasted_thread_id("  demo_upload\n", known).as_deref(), Some("demo_upload"));
        assert_eq!(pasted_thread_id("thread:demo_upload", known).as_deref(), Some("demo_upload"));
        assert_eq!(pasted_thread_id("Threadbear://thread/demo_upload", known).as_deref(), Some("demo_upload"));
        assert_eq!(pasted_thread_id("0CF430AC-F429-4B1C", known).as_deref(), Some("0cf430ac-f429-4b1c"), "an id in capitals is the same thread");
        assert_eq!(pasted_thread_id("someone-elses-id", known), None, "an id no thread has stays words");
        assert_eq!(pasted_thread_id("see demo_upload", known), None, "words with an id in them stay words");
        assert_eq!(pasted_thread_id("", known), None);
        assert_eq!(pasted_thread_id(&"a".repeat(65), |_| true), None);
    }

    #[test]
    fn a_picture_still_coming_in_is_something_in_the_box() {
        let chip = "\u{FFFC}";
        // Only a picture that's still being copied in: no words, no picture
        // in the draft yet, but a restart would lose it.
        let importing = [(0..chip.len(), BoxChip::Picture(None))];
        assert_eq!(draft_in(chip, &importing, 0..chip.len()), (String::new(), vec![]));
        assert!(holds_anything(chip, &importing));
        assert!(holds_anything("hello", &[]));
        assert!(!holds_anything("  \n", &[]));
        assert!(!holds_anything("", &[]));
    }

    #[test]
    fn a_draft_cuts_into_words_pictures_and_mentions() {
        let text = "look \u{FFFC} at [Fix it](https://www.notion.so/Fix-it-abc) and [Brief](thread:t1)\u{FFFC}\u{FFFC}";
        let pieces = draft_pieces(text, 2);
        assert_eq!(pieces[0], DraftPiece::Words("look ".into()));
        assert_eq!(pieces[1], DraftPiece::Picture(0));
        assert_eq!(pieces[2], DraftPiece::Words(" at ".into()));
        assert!(matches!(&pieces[3], DraftPiece::Mention(m, link) if m.kind == MentionKind::Card && m.title == "Fix it" && link == "[Fix it](https://www.notion.so/Fix-it-abc)"));
        assert_eq!(pieces[4], DraftPiece::Words(" and ".into()));
        assert!(matches!(&pieces[5], DraftPiece::Mention(m, link) if m.kind == MentionKind::Thread && m.thread_id() == Some("t1") && link == "[Brief](thread:t1)"));
        // The third mark has no picture left: it goes.
        assert_eq!(pieces[6], DraftPiece::Picture(1));
        assert_eq!(pieces.len(), 7);
        // A paste of words: its marks go, and plain words stay one piece.
        assert_eq!(draft_pieces("a\u{FFFC}b", 0), vec![DraftPiece::Words("ab".into())]);
        assert!(draft_pieces("", 0).is_empty());
        assert!(has_mentions("[x](thread:abc)"));
        assert!(!has_mentions("[x](https://example.com/abc)"));
    }

    #[test]
    fn the_draft_in_a_selection() {
        let picture = ImageRef::new("/media/a.png", "image/png");
        let link = "[Brief](thread:t1)";
        let text = format!("see \u{FFFC} and {link} \u{FFFC}!");
        let first = 4..7;
        let mention = text.find(link).unwrap()..text.find(link).unwrap() + link.len();
        let second = mention.end + 1..mention.end + 4;
        let chips = vec![(first.clone(), BoxChip::Picture(Some(picture.clone()))), (mention.clone(), BoxChip::Mention), (second.clone(), BoxChip::Picture(None))];
        let (draft, images) = draft_in(&text, &chips, 0..text.len());
        assert_eq!(draft, format!("see \u{FFFC} and {link} !"), "a picture still copying in isn't in it");
        assert_eq!(images, vec![picture.clone()]);
        // Part of the way into the first chip counts it whole.
        let (draft, images) = draft_in(&text, &chips, 5..mention.end);
        assert_eq!(draft, format!("\u{FFFC} and {link}"));
        assert_eq!(images.len(), 1);
        // Words only, with a stray mark in them.
        let (draft, images) = draft_in("a\u{FFFC}b", &[], 0..5);
        assert_eq!(draft, "ab");
        assert!(images.is_empty());
        assert_eq!(draft_in(&text, &chips, 2..2).0, "");
    }

    #[test]
    fn a_clip_round_trips_with_its_words() {
        let picture = ImageRef { path: "/media/a.png".into(), media_type: "image/png".into(), width: Some(40), height: Some(30) };
        let clip = Clip::new("look \u{FFFC} and \u{FFFC}".into(), vec![picture.clone(), picture.clone()]);
        assert_eq!(clip.plain, "look [Picture 1] and [Picture 2]");
        let back = Clip::from_clipboard(&clip.plain, &clip.metadata()).expect("a clip");
        assert_eq!(back, clip);
        assert!(Clip::from_clipboard("other words", &clip.metadata()).is_none(), "words changed since: not this clip");
        assert!(Clip::from_clipboard(&clip.plain, &json!([{ "indent": 0 }])).is_none(), "another app's note");
        assert!(Clip::from_clipboard(&clip.plain.replace('\n', "\r\n"), &clip.metadata()).is_some());
    }
}
